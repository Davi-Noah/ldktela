import { RemoteAudioTrack, RemoteVideoTrack, Room, RoomEvent, Track } from 'livekit-client';
import type { RemoteParticipant, RemoteTrackPublication } from 'livekit-client';
import { type ApiClient, ApiError } from '../api/client';
import type { Snowflake } from '../api/types/Snowflake';
import { STATS_SAMPLE_INTERVAL_MS } from '../config';
import { backoffDelayMs } from '../gateway/backoff';
import { describeError, log } from '../log';
import { type PublishPreset, type QualityChoice, useMediaStore } from '../store/media';
import type { CaptureRequest, ScreenCapture, SenderSample } from './tracks';
import {
  applyQuality,
  captureScreen,
  publishScreen,
  readSenderStats,
  stopCapture,
  unpublishScreen,
} from './tracks';

/** Everything we hold for one remote screen, keyed by publisher identity. */
interface RemoteScreen {
  video: RemoteVideoTrack | null;
  audio: RemoteAudioTrack | null;
  publication: RemoteTrackPublication | null;
  videoElement: HTMLVideoElement | null;
  audioElement: HTMLAudioElement | null;
}

function emptyScreen(): RemoteScreen {
  return {
    video: null,
    audio: null,
    publication: null,
    videoElement: null,
    audioElement: null,
  };
}

/**
 * Owns the LiveKit room. Joining is never a user action: the gateway says which
 * Discord voice channel we are in and this follows it (ADR-0011).
 *
 * Holds **N** remote screens, not one (RF-31). Layer selection is left to
 * `adaptiveStream` by default: a video rendered small in the grid gets the low
 * layer on its own, and that is what keeps the egress of N screens from
 * multiplying by N (RF-32). Do not replace it with a fixed layer.
 */
export class MediaSession {
  private readonly api: ApiClient;
  private room: Room | null = null;
  private channelId: Snowflake | null = null;
  private canPublish = false;
  private capture: ScreenCapture | null = null;
  private readonly remotes = new Map<string, RemoteScreen>();
  private statsTimer: ReturnType<typeof setInterval> | null = null;
  private lastSample: SenderSample | null = null;
  private rejoinTimer: ReturnType<typeof setTimeout> | null = null;
  private rejoinAttempt = 0;

  constructor(api: ApiClient) {
    this.api = api;
  }

  private screen(identity: string): RemoteScreen {
    const existing = this.remotes.get(identity);
    if (existing !== undefined) {
      return existing;
    }
    const created = emptyScreen();
    this.remotes.set(identity, created);
    return created;
  }

  /**
   * The video element of one screen. Created once per screen and never
   * remounted (CLAUDE.md §7) — moving it into the picture-in-picture window
   * keeps the same element, and therefore the same decoder.
   */
  registerVideoElement(identity: string, element: HTMLVideoElement | null): void {
    const screen = this.screen(identity);
    if (screen.videoElement !== null && screen.video !== null) {
      screen.video.detach(screen.videoElement);
    }
    screen.videoElement = element;
    if (element !== null && screen.video !== null) {
      screen.video.attach(element);
    }
  }

  registerAudioElement(identity: string, element: HTMLAudioElement | null): void {
    const screen = this.screen(identity);
    if (screen.audioElement !== null && screen.audio !== null) {
      screen.audio.detach(screen.audioElement);
    }
    screen.audioElement = element;
    if (element !== null) {
      element.volume = useMediaStore.getState().screens[identity]?.volume ?? 1;
      if (screen.audio !== null) {
        screen.audio.attach(element);
      }
    }
  }

  async follow(channelId: Snowflake | null): Promise<void> {
    if (channelId === this.channelId) {
      return;
    }
    await this.leave();
    this.channelId = channelId;
    if (channelId !== null) {
      await this.connect(false);
    }
  }

  async leave(): Promise<void> {
    this.clearRejoin();
    this.stopStatsSampling();
    this.channelId = null;
    this.canPublish = false;
    const room = this.room;
    this.room = null;
    this.detachAll();
    if (this.capture !== null) {
      stopCapture(this.capture);
      this.capture = null;
    }
    if (room !== null) {
      room.removeAllListeners();
      await room.disconnect(true);
    }
    useMediaStore.getState().reset();
  }

  async startShare(request: CaptureRequest): Promise<void> {
    const store = useMediaStore.getState();
    if (this.channelId === null || this.capture !== null) {
      return;
    }
    store.setError(null);
    store.setStarting(true);

    let capture: ScreenCapture;
    log.info('compartilhamento: abrindo o seletor de tela', {
      superficie: request.surface,
      audio: request.audio,
      preset: request.preset,
    });
    try {
      // The OS picker runs first: it is the slow part, and a user who cancels it
      // must not cost an admission slot.
      capture = await captureScreen(request);
      log.info('compartilhamento: tela capturada', {
        temAudio: capture.audio !== null,
        trilha: capture.video.mediaStreamTrack.label,
      });
    } catch (error) {
      log.error('compartilhamento: captura falhou', error);
      store.setStarting(false);
      store.setError(captureMessage(error));
      return;
    }

    try {
      if (!this.canPublish) {
        log.debug('compartilhamento: reconectando com token de publicação');
        await this.connect(true);
      }
      const room = this.room;
      if (room === null) {
        throw new Error('a sala não está conectada');
      }
      log.debug('compartilhamento: publicando trilhas no LiveKit');
      await publishScreen(room.localParticipant, capture, request.preset);
      log.info('compartilhamento: no ar');
      this.capture = capture;
      capture.video.mediaStreamTrack.addEventListener(
        'ended',
        () => {
          // The user pressed the browser's own stop-sharing control.
          void this.stopShare();
        },
        { once: true },
      );
      store.setPublishing(true, capture.audio !== null);
      this.startStatsSampling();
      this.syncViewers();
    } catch (error) {
      log.error('compartilhamento: publicação falhou', error);
      stopCapture(capture);
      store.setStarting(false);
      store.setError(publishMessage(error));
    }
  }

  async stopShare(): Promise<void> {
    const capture = this.capture;
    this.capture = null;
    this.stopStatsSampling();
    useMediaStore.getState().setPublishing(false, false);
    if (capture === null) {
      return;
    }
    const room = this.room;
    if (room !== null) {
      await unpublishScreen(room.localParticipant, capture);
    }
    stopCapture(capture);
  }

  /**
   * Republishes with a different ladder (RF-36). Changing resolution or frame
   * rate cannot be negotiated in place — the track is replaced, and the caller
   * has to say so instead of letting the UI look frozen.
   */
  async changePreset(preset: PublishPreset): Promise<void> {
    useMediaStore.getState().setPublishPreset(preset);
    const capture = this.capture;
    if (capture === null) {
      return;
    }
    log.info('compartilhamento: trocando o preset', { preset });
    await this.stopShare();
    await this.startShare({ surface: capture.surface, audio: capture.audio !== null, preset });
  }

  setVolume(identity: string, volume: number): void {
    useMediaStore.getState().setVolume(identity, volume);
    const element = this.remotes.get(identity)?.audioElement;
    if (element != null) {
      element.volume = useMediaStore.getState().screens[identity]?.volume ?? volume;
    }
  }

  setQuality(identity: string, choice: QualityChoice): void {
    useMediaStore.getState().setQuality(identity, choice);
    const publication = this.remotes.get(identity)?.publication;
    if (publication != null) {
      applyQuality(publication, choice);
    }
  }

  private async connect(publish: boolean): Promise<void> {
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

    log.debug('sala: pedindo token', { canal: channelId, publicar: publish });
    let credentials;
    try {
      credentials = await this.api.roomToken(channelId, publish);
    } catch (error) {
      log.error('sala: o servidor recusou o token', error, { canal: channelId });
      store.setConnection('failed');
      throw error;
    }
    log.debug('sala: token recebido', { url: credentials.url, sala: credentials.room });

    const room = new Room({
      // RF-32: both are mandatory. A screen rendered small in the grid gets the
      // low layer, and one that is not visible gets nothing.
      adaptiveStream: true,
      dynacast: true,
      stopLocalTrackOnUnpublish: false,
    });
    this.wire(room);
    this.room = room;
    this.canPublish = publish;
    try {
      await room.connect(credentials.url, credentials.token);
    } catch (error) {
      log.error('sala: LiveKit recusou a conexão', error, { url: credentials.url });
      this.room = null;
      this.canPublish = false;
      store.setConnection('failed');
      throw error;
    }
    log.info('sala: conectado ao LiveKit', { sala: credentials.room, publicar: publish });
    store.setConnection('connected');
    this.adoptExistingTracks(room);
    this.syncViewers();
  }

  private wire(room: Room): void {
    room.on(RoomEvent.TrackSubscribed, (_track, publication, participant) => {
      this.adoptPublication(publication, participant);
    });
    room.on(RoomEvent.TrackUnsubscribed, (_track, publication, participant) => {
      this.dropPublication(publication, participant);
    });
    room.on(RoomEvent.ParticipantConnected, () => {
      this.syncViewers();
    });
    room.on(RoomEvent.ParticipantDisconnected, (participant) => {
      this.forget(participant.identity);
      this.syncViewers();
    });
    room.on(RoomEvent.Reconnecting, () => {
      useMediaStore.getState().setConnection('reconnecting');
    });
    room.on(RoomEvent.Reconnected, () => {
      useMediaStore.getState().setConnection('connected');
      this.syncViewers();
    });
    room.on(RoomEvent.Disconnected, () => {
      if (this.room === room) {
        this.scheduleRejoin();
      }
    });
  }

  private adoptExistingTracks(room: Room): void {
    for (const participant of room.remoteParticipants.values()) {
      for (const publication of participant.trackPublications.values()) {
        this.adoptPublication(publication, participant);
      }
    }
  }

  private adoptPublication(
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ): void {
    const track = publication.track;
    const identity = participant.identity;
    const store = useMediaStore.getState();

    if (publication.source === Track.Source.ScreenShare && track instanceof RemoteVideoTrack) {
      const screen = this.screen(identity);
      screen.video = track;
      screen.publication = publication;
      if (screen.videoElement !== null) {
        track.attach(screen.videoElement);
      }
      store.addScreen(identity, 'video');
      applyQuality(publication, store.screens[identity]?.quality ?? 'auto');
      log.info('sala: tela recebida', { de: identity });
      this.syncViewers();
      return;
    }

    if (publication.source === Track.Source.ScreenShareAudio && track instanceof RemoteAudioTrack) {
      const screen = this.screen(identity);
      screen.audio = track;
      if (screen.audioElement !== null) {
        track.attach(screen.audioElement);
        screen.audioElement.volume = store.screens[identity]?.volume ?? 1;
      }
      store.addScreen(identity, 'audio');
    }
  }

  private dropPublication(
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ): void {
    const identity = participant.identity;
    const screen = this.remotes.get(identity);
    if (screen === undefined) {
      return;
    }
    if (publication.source === Track.Source.ScreenShare) {
      if (screen.video !== null && screen.videoElement !== null) {
        screen.video.detach(screen.videoElement);
      }
      screen.video = null;
      screen.publication = null;
      useMediaStore.getState().removeScreen(identity, 'video');
      this.syncViewers();
      return;
    }
    if (publication.source === Track.Source.ScreenShareAudio) {
      if (screen.audio !== null && screen.audioElement !== null) {
        screen.audio.detach(screen.audioElement);
      }
      screen.audio = null;
      useMediaStore.getState().removeScreen(identity, 'audio');
    }
  }

  /** Everything belonging to one publisher is gone. */
  private forget(identity: string): void {
    const screen = this.remotes.get(identity);
    if (screen === undefined) {
      return;
    }
    if (screen.video !== null && screen.videoElement !== null) {
      screen.video.detach(screen.videoElement);
    }
    if (screen.audio !== null && screen.audioElement !== null) {
      screen.audio.detach(screen.audioElement);
    }
    this.remotes.delete(identity);
    const store = useMediaStore.getState();
    store.removeScreen(identity, 'video');
    store.removeScreen(identity, 'audio');
  }

  private detachAll(): void {
    for (const identity of [...this.remotes.keys()]) {
      this.forget(identity);
    }
  }

  private syncViewers(): void {
    const room = this.room;
    if (room === null) {
      useMediaStore.getState().setViewerIds([]);
      return;
    }
    const viewers: string[] = [];
    for (const participant of room.remoteParticipants.values()) {
      if (participant.getTrackPublication(Track.Source.ScreenShare) === undefined) {
        viewers.push(participant.identity);
      }
    }
    useMediaStore.getState().setViewerIds(viewers);
  }

  private startStatsSampling(): void {
    this.stopStatsSampling();
    this.lastSample = null;
    this.statsTimer = setInterval(() => {
      void this.sampleStats();
    }, STATS_SAMPLE_INTERVAL_MS);
  }

  private async sampleStats(): Promise<void> {
    const capture = this.capture;
    if (capture === null) {
      return;
    }
    try {
      const layers = await capture.video.getSenderStats();
      const reading = readSenderStats(layers, this.lastSample);
      this.lastSample = reading.sample;
      useMediaStore.getState().setStats(reading.stats);
    } catch {
      // A stats read that fails is not worth surfacing; the next tick tries again.
    }
  }

  private stopStatsSampling(): void {
    if (this.statsTimer !== null) {
      clearInterval(this.statsTimer);
      this.statsTimer = null;
    }
    this.lastSample = null;
  }

  private scheduleRejoin(): void {
    if (this.channelId === null || this.rejoinTimer !== null) {
      return;
    }
    const store = useMediaStore.getState();
    store.setConnection('reconnecting');
    const wantsPublish = this.canPublish;
    const delay = backoffDelayMs(this.rejoinAttempt);
    this.rejoinAttempt += 1;
    this.rejoinTimer = setTimeout(() => {
      this.rejoinTimer = null;
      // A fresh token, not the old one: the media token can simply have expired.
      void this.connect(wantsPublish).then(
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

function captureMessage(error: unknown): string | null {
  if (error instanceof DOMException && error.name === 'NotAllowedError') {
    // Cancelling the picker is not an error worth showing.
    return null;
  }
  return `Não foi possível capturar a tela. (${describeError(error)})`;
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
