import { RemoteAudioTrack, RemoteVideoTrack, Room, RoomEvent, Track } from 'livekit-client';
import type { RemoteParticipant, RemoteTrackPublication } from 'livekit-client';
import { type ApiClient, ApiError } from '../api/client';
import type { Snowflake } from '../api/types/Snowflake';
import { STATS_SAMPLE_INTERVAL_MS } from '../config';
import { backoffDelayMs } from '../gateway/backoff';
import { describeError, log } from '../log';
import {
  ownerOf,
  PUBLISHER_SUFFIX,
  type PublisherStats,
  type PublishPreset,
  type QualityChoice,
  shouldSilenceOtherScreens,
  useMediaStore,
} from '../store/media';
import {
  ownerOfPublication,
  publicationId,
  SOURCE_ORDER,
  sourceOfPublication,
  type PublicationId,
  type PublicationSource,
} from './publication';
import { useSessionStore } from '../store/session';
import { useUiStore } from '../store/ui';
import {
  type CameraDevice,
  listCameras,
  listShareSources,
  onShareEnded,
  setCameraPreview,
  setSharePreview,
  setTraySharing,
  type ShareSource,
  shareThumbnail,
  type SourceKind,
  startNativeCamera,
  startNativeShare,
  stopNativeCamera,
  stopNativeShare,
} from './native';
import { clearPreview } from './preview';
import { invoke } from '@tauri-apps/api/core';
import { applyQuality, DUPLICATE_IDENTITY_MESSAGE, shouldRejoin } from './tracks';

/** Everything we hold for one remote publication, keyed by its id (ADR-0038). */
interface RemotePublication {
  video: RemoteVideoTrack | null;
  audio: RemoteAudioTrack | null;
  publication: RemoteTrackPublication | null;
  videoElement: HTMLVideoElement | null;
  audioElement: HTMLAudioElement | null;
}

function emptyPublication(): RemotePublication {
  return {
    video: null,
    audio: null,
    publication: null,
    videoElement: null,
    audioElement: null,
  };
}

/**
 * Qual publicação uma trilha do LiveKit alimenta.
 *
 * O áudio pertence à tela: ele é o som do que está sendo compartilhado, e a
 * câmera nunca carrega áudio nenhum (ADR-0038). `null` para qualquer outra
 * fonte — microfone não existe neste produto, e uma trilha inesperada é
 * ignorada em vez de virar um ladrilho fantasma.
 */
function sourceOfTrack(source: Track.Source): PublicationSource | null {
  switch (source) {
    case Track.Source.ScreenShare:
    case Track.Source.ScreenShareAudio:
      return 'screen';
    case Track.Source.Camera:
      return 'camera';
    default:
      return null;
  }
}

/** What the core needs to start a share; remembered so a preset change can redo it. */
export interface ShareChoice {
  sourceId: string;
  kind: SourceKind;
  audio: boolean;
  /**
   * What the picker called it. Never reaches the core — it exists so the
   * interface can say *what* is being shared, in the tray tooltip and over the
   * preview.
   */
  title: string;
}

/**
 * Owns the LiveKit room the app **watches** with. Joining is never a user
 * action: the gateway says which Discord voice channel we are in and this
 * follows it (ADR-0011).
 *
 * Publishing is not here any more. It lives in the Rust core, on its own
 * connection with its own identity (ADR-0026, ADR-0027), which is why starting
 * a share no longer tears this connection down — it used to reconnect just to
 * swap the token, dropping every subscription and decoder on the way.
 *
 * Holds **N** remote screens, not one (RF-31). Layer selection is left to
 * `adaptiveStream` by default. Desde o ADR-0032 a escada é temporal, não
 * espacial, então o que ele economiza é o ladrilho **escondido**, que deixa de
 * ser assinado — e não mais o ladrilho pequeno, que passou a receber a resolução
 * cheia por não haver outra. Mesmo assim, não troque por uma camada fixa: é a
 * metade do RF-32 que sobrou.
 */
export class MediaSession {
  private readonly api: ApiClient;
  private room: Room | null = null;
  private channelId: Snowflake | null = null;
  private sharing: ShareChoice | null = null;
  private readonly remotes = new Map<PublicationId, RemotePublication>();
  private statsTimer: ReturnType<typeof setInterval> | null = null;
  private rejoinTimer: ReturnType<typeof setTimeout> | null = null;
  private rejoinAttempt = 0;

  constructor(api: ApiClient) {
    this.api = api;
    void onShareEnded(({ source, reason }) => {
      // O core parou sem nos pedir: o SFU derrubou, a janela compartilhada foi
      // fechada, a câmera foi desconectada. Sem isto o botão continuaria
      // dizendo "parar".
      //
      // Só a fonte que acabou: uma câmera desconectada não pode apagar a tela
      // que continua no ar (ADR-0038).
      log.warn('transmissão: encerrada pelo sistema', { motivo: reason, fonte: source });
      const store = useMediaStore.getState();
      if (source === 'camera') {
        if (!store.camera.publishing) {
          return;
        }
        store.setCameraPublishing(null);
        clearPreview('camera');
        // O motivo vem junto, e não é enfeite: uma câmera que morre sozinha sem
        // dizer por quê não deixa nem quem usa nem nós com o que trabalhar.
        useUiStore.getState().toast('warning', `A câmera foi encerrada: ${reason}`);
        this.stopStatsSamplingIfIdle();
        return;
      }
      if (this.sharing === null) {
        return;
      }
      this.sharing = null;
      store.setPublishing(false, false);
      // Aviso e não erro: fechar a janela que estava sendo compartilhada é um
      // fim normal, e pintar isso de vermelho ensina o usuário a ignorar
      // vermelho.
      useUiStore.getState().toast('warning', 'O compartilhamento foi encerrado.');
      clearPreview('screen');
      setTraySharing(false, null);
      this.applyAudioPolicy();
      this.stopStatsSamplingIfIdle();
    });
  }

  /** The list the picker shows (RF-37). Enumerated by the core, not by Chromium. */
  listSources(): Promise<ShareSource[]> {
    return listShareSources();
  }

  /** One card's picture (RF-37). Asked for per source, as the picker draws them. */
  thumbnail(kind: SourceKind, sourceId: string): Promise<string | null> {
    return shareThumbnail(kind, sourceId);
  }

  /**
   * Liga, desliga e acelera o preview da própria tela (ADR-0030).
   *
   * Desligado, o ramo para dentro da thread de captura — é a mesma disciplina
   * do `adaptiveStream` para as telas dos outros: o que não está sendo olhado
   * não é produzido.
   */
  async setPreview(
    source: PublicationSource,
    enabled: boolean,
    fps: number,
    focused: boolean,
  ): Promise<void> {
    try {
      await (source === 'camera'
        ? setCameraPreview(enabled, fps, focused)
        : setSharePreview(enabled, fps, focused));
    } catch (error) {
      log.debug('preview: o core recusou o ajuste', { error, fonte: source });
    }
  }

  private held(id: PublicationId): RemotePublication {
    const existing = this.remotes.get(id);
    if (existing !== undefined) {
      return existing;
    }
    const created = emptyPublication();
    this.remotes.set(id, created);
    return created;
  }

  /**
   * The video element of one publication. Created once and never remounted
   * (CLAUDE.md §7) — moving it into the picture-in-picture window keeps the same
   * element, and therefore the same decoder.
   */
  registerVideoElement(id: PublicationId, element: HTMLVideoElement | null): void {
    const held = this.held(id);
    if (held.videoElement !== null && held.video !== null) {
      held.video.detach(held.videoElement);
    }
    held.videoElement = element;
    if (element !== null && held.video !== null) {
      held.video.attach(element);
    }
  }

  registerAudioElement(id: PublicationId, element: HTMLAudioElement | null): void {
    const held = this.held(id);
    if (held.audioElement !== null && held.audio !== null) {
      held.audio.detach(held.audioElement);
    }
    held.audioElement = element;
    if (element !== null) {
      if (held.audio !== null) {
        held.audio.attach(element);
      }
      this.applyAudioPolicy();
    }
  }

  async follow(channelId: Snowflake | null): Promise<void> {
    if (channelId === this.channelId) {
      return;
    }
    await this.leave();
    this.channelId = channelId;
    if (channelId !== null) {
      await this.connect();
    }
  }

  async leave(): Promise<void> {
    this.clearRejoin();
    this.stopStatsSampling();
    this.channelId = null;
    const room = this.room;
    this.room = null;
    this.detachAll();
    if (this.sharing !== null) {
      this.sharing = null;
      clearPreview('screen');
      setTraySharing(false, null);
      await stopNativeShare().catch((error: unknown) => {
        log.error('compartilhamento: falha ao parar', error);
      });
    }
    if (room !== null) {
      room.removeAllListeners();
      await room.disconnect(true);
    }
    useMediaStore.getState().reset();
  }

  /**
   * Tudo o que vamos ter no ar, e não só o que está começando (ADR-0038).
   *
   * O servidor lê a lista como a intenção inteira: o que ficar de fora perde a
   * vaga. Pedir um token só para a câmera enquanto a tela transmite devolveria
   * a vaga da tela e deixaria outra pessoa tomá-la.
   */
  private publishIntent(adding?: PublicationSource): PublicationSource[] {
    const state = useMediaStore.getState();
    const live = new Set<PublicationSource>();
    if (state.publishing) {
      live.add('screen');
    }
    if (state.camera.publishing) {
      live.add('camera');
    }
    if (adding !== undefined) {
      live.add(adding);
    }
    return SOURCE_ORDER.filter((source) => live.has(source));
  }

  /**
   * Starts a share in the core.
   *
   * The publish token is fetched here and handed over, rather than letting the
   * core talk to our API: the session already lives on this side, and a second
   * copy of authentication would be a second place for it to go wrong. It is
   * also what keeps the admission guard (RF-13) working unchanged — the server
   * still decides, against the authenticated user.
   */
  async startShare(choice: ShareChoice, preset: PublishPreset): Promise<void> {
    const store = useMediaStore.getState();
    const channelId = this.channelId;
    if (channelId === null || this.sharing !== null) {
      return;
    }
    store.setStarting(true);
    log.info('compartilhamento: iniciando', { ...choice, preset });

    let credentials;
    try {
      credentials = await this.api.roomToken(channelId, this.publishIntent('screen'));
    } catch (error) {
      log.error('compartilhamento: o servidor recusou o token', error);
      store.setStarting(false);
      useUiStore.getState().toast('danger', publishMessage(error));
      return;
    }

    try {
      const started = await startNativeShare({
        url: credentials.url,
        token: credentials.token,
        sourceId: choice.sourceId,
        kind: choice.kind,
        preset,
        audio: choice.audio,
      });
      this.sharing = choice;
      store.setPublishing(true, started.audio !== null, started.audio, choice.title);
      setTraySharing(true, choice.title);
      log.info('compartilhamento: no ar', { audio: started.audio, fonte: choice.title });
      if (started.audio === 'whole_system') {
        log.warn('compartilhamento: sem Discord para excluir, indo o sistema inteiro');
      }
      this.applyAudioPolicy();
      this.startStatsSampling();
    } catch (error) {
      log.error('compartilhamento: o core recusou', error);
      store.setStarting(false);
      useUiStore.getState().toast('danger', publishMessage(error));
    }
  }

  /**
   * Switches source, audio or preset without the user stopping first (issue #9).
   *
   * Parar e recomeçar continua sendo o que acontece por baixo — a trilha é
   * substituída, e trocar de fonte não é renegociável no lugar (ver
   * `changePreset`). O que muda é de quem é o trabalho: quem quer passar da tela
   * 1 para a 2 não precisa mais parar, reabrir o seletor e começar de novo.
   */
  async switchShare(choice: ShareChoice, preset: PublishPreset): Promise<void> {
    if (this.sharing !== null) {
      await this.stopShare();
    }
    await this.startShare(choice, preset);
  }

  async stopShare(): Promise<void> {
    if (this.sharing === null) {
      return;
    }
    this.sharing = null;
    this.stopStatsSampling();
    useMediaStore.getState().setPublishing(false, false);
    clearPreview('screen');
    setTraySharing(false, null);
    this.applyAudioPolicy();
    try {
      await stopNativeShare();
    } catch (error) {
      log.error('compartilhamento: falha ao parar', error);
    }
  }

  /** As cameras que o core enxerga, para o seletor (ADR-0038). */
  listCameras(): Promise<CameraDevice[]> {
    return listCameras();
  }

  /**
   * Liga a camera, por cima da tela se ela ja estiver no ar (ADR-0038).
   *
   * O token e pedido com a intencao inteira: com a tela transmitindo, pedir um
   * token so de camera devolveria a vaga da tela no servidor.
   */
  async startCamera(device: CameraDevice): Promise<void> {
    const store = useMediaStore.getState();
    const channelId = this.channelId;
    if (channelId === null || store.camera.publishing) {
      return;
    }
    store.setCameraStarting(true);
    log.info('camera: iniciando', { dispositivo: device.name });

    let credentials;
    try {
      credentials = await this.api.roomToken(channelId, this.publishIntent('camera'));
    } catch (error) {
      log.error('camera: o servidor recusou o token', error);
      store.setCameraStarting(false);
      useUiStore.getState().toast('danger', publishMessage(error));
      return;
    }

    try {
      await startNativeCamera({
        url: credentials.url,
        token: credentials.token,
        deviceId: device.id,
      });
      store.setCameraPublishing({ id: device.id, name: device.name });
      log.info('camera: no ar', { dispositivo: device.name });
      this.startStatsSampling();
    } catch (error) {
      log.error('camera: o core recusou', error);
      store.setCameraStarting(false);
      // A mensagem do core diz o que aconteceu — camera ocupada, bloqueada pelo
      // Windows, desconectada — e e ela que a pessoa precisa ler.
      useUiStore.getState().toast('danger', cameraMessage(error));
    }
  }

  async stopCamera(): Promise<void> {
    const store = useMediaStore.getState();
    if (!store.camera.publishing) {
      return;
    }
    store.setCameraPublishing(null);
    clearPreview('camera');
    try {
      await stopNativeCamera();
    } catch (error) {
      log.error('camera: falha ao parar', error);
    }
    this.stopStatsSamplingIfIdle();
  }

  /** Troca de camera sem passar por "desligada": o ladrilho nao pisca. */
  async switchCamera(device: CameraDevice): Promise<void> {
    if (useMediaStore.getState().camera.publishing) {
      await this.stopCamera();
    }
    await this.startCamera(device);
  }

  /**
   * Republishes with a different ladder (RF-36). Resolution and frame rate
   * cannot be renegotiated in place — the track is replaced, and the interface
   * warns that it will blink.
   */
  async changePreset(preset: PublishPreset): Promise<void> {
    useMediaStore.getState().setPublishPreset(preset);
    const choice = this.sharing;
    if (choice === null) {
      return;
    }
    log.info('compartilhamento: trocando o preset', { preset });
    await this.stopShare();
    await this.startShare(choice, preset);
  }

  setVolume(owner: string, volume: number): void {
    useMediaStore.getState().setVolume(owner, volume);
    this.applyAudioPolicy();
  }

  /**
   * Leaves or re-enters one person's screen (issue #6, ADR-0036).
   *
   * Sair é deixar de assinar de verdade, e não esconder o ladrilho: o que custa
   * banda e decodificação é a trilha chegando, então escondê-la não devolveria
   * nada a quem saiu. O ladrilho fica, sem trilha, porque é dele que se volta.
   */
  setPublicationSubscribed(id: PublicationId, subscribed: boolean): void {
    const store = useMediaStore.getState();
    store.setSubscribed(id, subscribed);
    const owner = ownerOfPublication(id);
    const source = sourceOfPublication(id);
    if (!subscribed) {
      // O ladrilho sai do layout (ADR-0036), então continuar em foco nele
      // deixaria a janela inteira vazia.
      if (store.focused === id) {
        store.focus(null);
      }
      // Dito uma vez, e não desenhado para sempre: sem isto, a tela some e o
      // caminho de volta fica escondido atrás de um botão que ninguém abriu.
      useUiStore
        .getState()
        .toast(
          'info',
          source === 'camera'
            ? 'Saiu da câmera. Para voltar, abra a lista de pessoas no rodapé.'
            : 'Saiu da tela. Para voltar, abra a lista de pessoas no rodapé.',
        );
    }
    const room = this.room;
    if (room === null) {
      return;
    }
    for (const participant of room.remoteParticipants.values()) {
      if (ownerOf(participant.identity) !== owner) {
        continue;
      }
      for (const publication of participant.trackPublications.values()) {
        // Só as trilhas desta fonte: sair da câmera de alguém não pode
        // cancelar a assinatura da tela da mesma pessoa.
        if (sourceOfTrack(publication.source) === source) {
          publication.setSubscribed(subscribed);
        }
      }
    }
    log.info(subscribed ? 'sala: entrei numa publicação' : 'sala: saí de uma publicação', {
      de: owner,
      fonte: source,
    });
  }

  setQuality(id: PublicationId, choice: QualityChoice): void {
    useMediaStore.getState().setQuality(id, choice);
    const publication = this.remotes.get(id)?.publication;
    if (publication != null) {
      applyQuality(publication, choice);
    }
  }

  /**
   * ADR-0028. While we transmit audio, other people's screen audio is silenced
   * here, because it would otherwise be picked up by our own system capture and
   * sent back out.
   */
  private applyAudioPolicy(): void {
    const state = useMediaStore.getState();
    const silence = shouldSilenceOtherScreens(state);
    for (const [id, held] of this.remotes) {
      const element = held.audioElement;
      if (element === null) {
        continue;
      }
      element.muted = silence;
      element.volume = state.publications[id]?.volume ?? 1;
    }
  }

  private async connect(): Promise<void> {
    const channelId = this.channelId;
    if (channelId === null) {
      return;
    }
    const store = useMediaStore.getState();
    store.setConnection('connecting');

    const previous = this.room;
    this.room = null;
    this.detachAll();
    if (previous !== null) {
      previous.removeAllListeners();
      await previous.disconnect(false);
    }

    log.debug('sala: pedindo token de espectador', { canal: channelId });
    let credentials;
    try {
      credentials = await this.api.roomToken(channelId, []);
    } catch (error) {
      log.error('sala: o servidor recusou o token', error, { canal: channelId });
      store.setConnection('failed', 'unreachable');
      throw error;
    }

    const room = new Room({
      // RF-32: both are mandatory. A screen rendered small in the grid gets the
      // low layer, and one that is not visible gets nothing.
      adaptiveStream: true,
      dynacast: true,
      stopLocalTrackOnUnpublish: false,
    });
    this.wire(room);
    this.room = room;
    try {
      await room.connect(credentials.url, credentials.token);
    } catch (error) {
      log.error('sala: LiveKit recusou a conexão', error, { url: credentials.url });
      this.room = null;
      store.setConnection('failed', 'unreachable');
      throw error;
    }
    log.info('sala: conectado ao LiveKit', { sala: credentials.room });
    store.setConnection('connected');
    this.adoptExistingTracks(room);
    this.syncViewers();
  }

  private wire(room: Room): void {
    room.on(RoomEvent.TrackSubscribed, (_track, publication, participant) => {
      this.adoptPublication(publication, participant);
    });
    room.on(RoomEvent.TrackUnsubscribed, (_track, publication, participant) => {
      this.dropTrack(publication, participant);
    });
    room.on(RoomEvent.TrackUnpublished, (publication, participant) => {
      // Quem parou de transmitir leva o ladrilho junto, inclusive o de quem
      // tinha saído daquela tela: sem isto, sair de uma tela deixaria para
      // sempre um convite para entrar numa transmissão que acabou.
      //
      // Só o ladrilho daquela fonte: despublicar a câmera não pode apagar a
      // tela que continua no ar (ADR-0038). O áudio não conta — ele vai e volta
      // sozinho enquanto a tela segue.
      const source = sourceOfTrack(publication.source);
      if (source !== null && publication.source !== Track.Source.ScreenShareAudio) {
        this.forget(publicationId(ownerOf(participant.identity), source));
        this.syncViewers();
      }
    });
    room.on(RoomEvent.ParticipantConnected, () => {
      this.syncViewers();
    });
    room.on(RoomEvent.ParticipantDisconnected, (participant) => {
      this.forgetOwner(ownerOf(participant.identity));
      this.syncViewers();
    });
    room.on(RoomEvent.Reconnecting, () => {
      useMediaStore.getState().setConnection('reconnecting');
    });
    room.on(RoomEvent.Reconnected, () => {
      useMediaStore.getState().setConnection('connected');
      this.syncViewers();
    });
    room.on(RoomEvent.Disconnected, (reason) => {
      if (this.room !== room) {
        return;
      }
      if (!shouldRejoin(reason)) {
        // Reconectar aqui expulsaria a outra ponta, que reconectaria e nos
        // expulsaria: as duas trocariam a sala para sempre.
        log.error('sala: a mesma conta entrou de outro lugar', undefined, { reason });
        useMediaStore.getState().setConnection('failed', 'duplicate_identity');
        useUiStore.getState().toast('danger', DUPLICATE_IDENTITY_MESSAGE);
        return;
      }
      this.scheduleRejoin();
    });
  }

  private adoptExistingTracks(room: Room): void {
    for (const participant of room.remoteParticipants.values()) {
      for (const publication of participant.trackPublications.values()) {
        this.adoptPublication(publication, participant);
      }
    }
  }

  /** Our own publishing connection, which is a remote participant to this one. */
  private isOurOwnPublisher(identity: string): boolean {
    const me = useSessionStore.getState().user?.id;
    return me !== undefined && identity === `${me}${PUBLISHER_SUFFIX}`;
  }

  private adoptPublication(
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ): void {
    // Assinar a propria tela custaria egress e ingress para receber de volta o
    // que ja esta nesta maquina.
    if (this.isOurOwnPublisher(participant.identity)) {
      return;
    }
    const track = publication.track;
    const source = sourceOfTrack(publication.source);
    if (source === null) {
      return;
    }
    const owner = ownerOf(participant.identity);
    const id = publicationId(owner, source);
    const store = useMediaStore.getState();

    if (publication.source !== Track.Source.ScreenShareAudio && track instanceof RemoteVideoTrack) {
      const held = this.held(id);
      held.video = track;
      held.publication = publication;
      if (held.videoElement !== null) {
        track.attach(held.videoElement);
      }
      store.addTrack(id, 'video');
      applyQuality(publication, store.publications[id]?.quality ?? 'auto');
      log.info('sala: publicação recebida', { de: owner, fonte: source });
      this.syncViewers();
      return;
    }

    if (publication.source === Track.Source.ScreenShareAudio && track instanceof RemoteAudioTrack) {
      const held = this.held(id);
      held.audio = track;
      if (held.audioElement !== null) {
        track.attach(held.audioElement);
      }
      store.addTrack(id, 'audio');
      this.applyAudioPolicy();
    }
  }

  private dropTrack(publication: RemoteTrackPublication, participant: RemoteParticipant): void {
    const source = sourceOfTrack(publication.source);
    if (source === null) {
      return;
    }
    const id = publicationId(ownerOf(participant.identity), source);
    const held = this.remotes.get(id);
    if (held === undefined) {
      return;
    }
    if (publication.source === Track.Source.ScreenShareAudio) {
      if (held.audio !== null && held.audioElement !== null) {
        held.audio.detach(held.audioElement);
      }
      held.audio = null;
      useMediaStore.getState().removeTrack(id, 'audio');
      return;
    }
    if (held.video !== null && held.videoElement !== null) {
      held.video.detach(held.videoElement);
    }
    held.video = null;
    held.publication = null;
    useMediaStore.getState().removeTrack(id, 'video');
    this.syncViewers();
  }

  /** One publication is gone for good. */
  private forget(id: PublicationId): void {
    const held = this.remotes.get(id);
    if (held !== undefined) {
      if (held.video !== null && held.videoElement !== null) {
        held.video.detach(held.videoElement);
      }
      if (held.audio !== null && held.audioElement !== null) {
        held.audio.detach(held.audioElement);
      }
      this.remotes.delete(id);
    }
    // `dropPublication`, e não `removeTrack`: o ladrilho de uma tela da qual se
    // saiu sobrevive à perda das trilhas de propósito, e aqui acabou de vez.
    useMediaStore.getState().dropPublication(id);
  }

  /** Everything one person was transmitting is gone: as duas fontes vão junto. */
  private forgetOwner(owner: string): void {
    for (const id of [...this.remotes.keys()]) {
      if (ownerOfPublication(id) === owner) {
        this.forget(id);
      }
    }
    // Mesmo sem trilha nenhuma recebida, pode haver ladrilho de quem saiu da
    // tela (ADR-0036): o store guarda, e é aqui que ele sai.
    for (const source of ['screen', 'camera'] as const) {
      useMediaStore.getState().dropPublication(publicationId(owner, source));
    }
  }

  private detachAll(): void {
    for (const id of [...this.remotes.keys()]) {
      this.forget(id);
    }
  }

  /**
   * Who is watching.
   *
   * Publishing connections are skipped: they are not people, and counting them
   * would make every publisher show up as one of their own viewers.
   */
  private syncViewers(): void {
    const room = this.room;
    if (room === null) {
      useMediaStore.getState().setViewerIds([]);
      return;
    }
    const viewers: string[] = [];
    for (const participant of room.remoteParticipants.values()) {
      if (participant.identity.endsWith(PUBLISHER_SUFFIX)) {
        continue;
      }
      if (participant.getTrackPublication(Track.Source.ScreenShare) === undefined) {
        viewers.push(participant.identity);
      }
    }
    useMediaStore.getState().setViewerIds(viewers);
  }

  private startStatsSampling(): void {
    this.stopStatsSampling();
    this.statsTimer = setInterval(() => {
      void this.sampleStats();
    }, STATS_SAMPLE_INTERVAL_MS);
  }

  /** Sampled on an interval, never in the media path (CLAUDE.md §7). */
  private async sampleStats(): Promise<void> {
    const store = useMediaStore.getState();
    if (this.sharing !== null) {
      store.setStats(await readStats('share_stats'));
    }
    if (store.camera.publishing) {
      store.setCameraStats(await readStats('camera_stats'));
    }
  }

  /** Para a amostragem quando nenhuma das duas fontes está no ar. */
  private stopStatsSamplingIfIdle(): void {
    const store = useMediaStore.getState();
    if (this.sharing === null && !store.camera.publishing) {
      this.stopStatsSampling();
    }
  }

  private stopStatsSampling(): void {
    if (this.statsTimer !== null) {
      clearInterval(this.statsTimer);
      this.statsTimer = null;
    }
    const store = useMediaStore.getState();
    store.setStats(null);
    store.setCameraStats(null);
  }

  private scheduleRejoin(): void {
    if (this.channelId === null || this.rejoinTimer !== null) {
      return;
    }
    useMediaStore.getState().setConnection('reconnecting');
    const delay = backoffDelayMs(this.rejoinAttempt);
    this.rejoinAttempt += 1;
    this.rejoinTimer = setTimeout(() => {
      this.rejoinTimer = null;
      // A fresh token, not the old one: the media token can simply have expired.
      void this.connect().then(
        () => {
          this.rejoinAttempt = 0;
        },
        () => {
          this.scheduleRejoin();
        },
      );
    }, delay);
  }

  private clearRejoin(): void {
    if (this.rejoinTimer !== null) {
      clearTimeout(this.rejoinTimer);
      this.rejoinTimer = null;
    }
    this.rejoinAttempt = 0;
  }
}

/** The core's stats, in its own snake_case. */
interface NativeStats {
  bitrate_kbps: number;
  fps: number;
  width: number;
  height: number;
  hardware_encoder: boolean;
  limited_by: string;
  transport: string;
  rtt_ms: number;
  available_kbps: number;
  captured_frames: number;
  encoded_frames: number;
  audio_samples: number | null;
}

async function readStats(command: 'share_stats' | 'camera_stats'): Promise<PublisherStats | null> {
  try {
    const stats = await invoke<NativeStats | null>(command);
    if (stats === null) {
      return null;
    }
    return {
      bitrateKbps: stats.bitrate_kbps,
      fps: stats.fps,
      width: stats.width,
      height: stats.height,
      hardwareEncoder: stats.hardware_encoder,
      limitedBy: stats.limited_by,
      transport: stats.transport,
      rttMs: stats.rtt_ms,
      availableKbps: stats.available_kbps,
      capturedFrames: stats.captured_frames,
      encodedFrames: stats.encoded_frames,
      audioSamples: stats.audio_samples,
    };
  } catch {
    // Uma leitura que falha nao vale um erro na tela; o proximo tique tenta.
    return null;
  }
}

/**
 * O core já explica o que houve — câmera ocupada, bloqueada pelo Windows,
 * desconectada — e é essa frase que a pessoa precisa ler. Só o que não vem dele
 * ganha texto nosso.
 */
function cameraMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 409) {
      return 'A sala já está com o número máximo de câmeras ligadas.';
    }
    return publishMessage(error);
  }
  if (typeof error === 'string' && error.trim() !== '') {
    return error;
  }
  return `Não foi possível ligar a câmera. (${describeError(error)})`;
}

function publishMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 409) {
      return 'A sala já está com o número máximo de telas compartilhadas.';
    }
    if (error.status === 403 || error.status === 404) {
      return 'Você não pode compartilhar neste canal.';
    }
    return error.message;
  }
  return `Não foi possível iniciar o compartilhamento. (${describeError(error)})`;
}
