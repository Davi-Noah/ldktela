import { create } from 'zustand';
import type { AudioMode } from '../media/native';
import {
  ownerOfPublication,
  publicationId,
  SOURCE_ORDER,
  sourceOfPublication,
  type PublicationId,
  type PublicationSource,
} from '../media/publication';

export type MediaConnection = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'failed';

/**
 * Por que a conexão de mídia desistiu. `null` enquanto não desistiu.
 *
 * Existe porque `failed` sozinho vira uma sala de aparência normal — lista de
 * participantes, botão de compartilhar — sobre uma conexão que não existe. Quem
 * viu isso concluiu que o produto simplesmente não mostra a tela dos outros.
 *
 * `duplicate_identity` é o caso que todo mundo encontra na primeira semana: o
 * aplicativo instalado em duas máquinas na mesma conta do Discord. A segunda a
 * entrar expulsa a primeira, e a primeira fica numa sala que parece vazia.
 */
export type MediaFault = 'duplicate_identity' | 'unreachable';

/** Automatic follows the element size; the other two pin a simulcast layer. */
export type QualityChoice = 'auto' | 'high' | 'low';

/**
 * What the publisher encodes (RF-36). Resolution and frame rate belong to whoever
 * pays for the encode, not to the viewer — a viewer-side fps selector would be
 * offering combinations nobody is sending (ADR-0023).
 */
export type PublishPreset = '1080p60' | '1080p30' | '720p60' | '720p30';

export const PUBLISH_PRESETS: readonly PublishPreset[] = ['1080p60', '1080p30', '720p60', '720p30'];

export interface PublisherStats {
  bitrateKbps: number;
  fps: number;
  width: number;
  height: number;
  /** Whether libwebrtc picked a hardware encoder. Only observable since
      publishing moved to the core (ADR-0026). */
  hardwareEncoder: boolean;
  /** `none`, `cpu`, `bandwidth` ou `other`: o encoder dizendo por que se segura.
      Sem isso, rede congestionada e CPU insuficiente são indistinguíveis. */
  limitedBy: string;
  /** `udp`, `tcp`, `relay/udp`… Cair para TCP derruba a qualidade sozinho, e é
      sintoma de porta fechada, não de rede ruim. */
  transport: string;
  rttMs: number;
  availableKbps: number;
  /** Frames taken from the screen, and of those how many the encoder accepted.
      A wide gap means the stream is paused for want of a subscriber, which looks
      exactly like a dead capture from outside. */
  capturedFrames: number;
  encodedFrames: number;
  /** Samples per channel captured, or `null` when sharing without audio. Zero
      while sharing with audio means the capture opened but nothing is coming
      through — which sounds exactly like a muted game and is not the same. */
  audioSamples: number | null;
}

/**
 * One publication being received, keyed by `${ownerId}:${source}` (ADR-0038).
 *
 * Chaveado pela publicação, e não pela pessoa: quem transmite a tela e a câmera
 * ao mesmo tempo tem dois ladrilhos, e o volume, a qualidade e o "saí desta
 * tela" de um não podem alcançar o outro.
 */
export interface PublicationState {
  id: PublicationId;
  /** Quem transmite. É por aqui que se acha o nome e o avatar na sala. */
  ownerId: string;
  source: PublicationSource;
  hasVideo: boolean;
  hasAudio: boolean;
  /** 0 to 1. Independent per publication (RF-35) and kept across focus changes. */
  volume: number;
  quality: QualityChoice;
  /**
   * Whether this viewer wants this publication at all (ADR-0036, issue #6).
   *
   * `false` mantém o ladrilho sem trilha nenhuma: sem ele, sair de uma tela
   * apagaria o único lugar de onde se pode voltar a entrar.
   */
  subscribed: boolean;
}

/** O que estamos transmitindo pela câmera, se estivermos (ADR-0038). */
export interface CameraShare {
  publishing: boolean;
  /** O seletor está aberto ou o core ainda está abrindo o dispositivo. */
  starting: boolean;
  deviceId: string | null;
  /** Como o Windows chama o dispositivo, para o painel dizer o que está no ar. */
  deviceName: string | null;
  stats: PublisherStats | null;
}

const DEFAULT_VOLUME = 1;

export interface MediaState {
  connection: MediaConnection;
  /** Preenchido junto com `connection: 'failed'`, e limpo em qualquer outra. */
  fault: MediaFault | null;
  /** True from the moment our screen track is published until it is dropped. */
  publishing: boolean;
  /**
   * What we are sharing, in the words of the picker ("Tela 1", "Elden Ring").
   *
   * Kept because the app could not answer the question a publisher asks every
   * few minutes — "am I still showing the right thing?" — and a panel that says
   * "you are sharing" with no object is not an answer.
   */
  sharingTitle: string | null;
  /** True while the OS picker is open, so the button can say so. */
  starting: boolean;
  sharingAudio: boolean;
  /**
   * Which capture mode the core got when audio was requested (RF-30).
   * `whole_system` means everyone's Discord voice is going out with the screen,
   * and the interface has to say so rather than let the user find out from
   * their friends.
   */
  audioMode: AudioMode | null;
  publishPreset: PublishPreset;
  /**
   * Whether the next share goes out with the computer's sound.
   *
   * Ligado por padrão: compartilhar tela sem som surpreende — quem assiste
   * avisa que não há áudio, e quem transmite não sabe onde procurar. Mora aqui,
   * e não no seletor, para que desligar dure a sessão inteira: religar sozinho
   * mandaria áudio que a pessoa já tinha decidido não mandar.
   */
  shareAudio: boolean;
  /** A câmera é uma publicação à parte, com estado próprio (ADR-0038). */
  camera: CameraShare;
  /** Publications being received, by publication id (RF-31, ADR-0038). */
  publications: Record<PublicationId, PublicationState>;
  /** Arrival order, so the grid does not reshuffle on every render. */
  publicationOrder: PublicationId[];
  /** Publication shown large. `null` means the grid. */
  focused: PublicationId | null;
  /**
   * No foco, esconder as outras telas em vez de mostrá-las na lateral.
   *
   * É um tempero do foco, e não um terceiro modo solto: sair do foco volta para
   * a grade nos dois casos. Assistir a várias e assistir a uma são as duas
   * coisas que uma sala de telas precisa fazer, e a lateral serve à primeira.
   */
  solo: boolean;
  /** Publication currently in the picture-in-picture window (RF-33). */
  detached: PublicationId | null;
  /** Identities connected to the media room, minus ourselves: the viewers. */
  viewerIds: string[];
  stats: PublisherStats | null;
}

interface MediaStore extends MediaState {
  setConnection: (connection: MediaConnection, fault?: MediaFault | null) => void;
  setPublishing: (
    publishing: boolean,
    sharingAudio: boolean,
    audioMode?: AudioMode | null,
    title?: string | null,
  ) => void;
  setStarting: (starting: boolean) => void;
  setPublishPreset: (preset: PublishPreset) => void;
  setShareAudio: (shareAudio: boolean) => void;
  /** A câmera entrou ou saiu do ar. `null` no dispositivo é parar. */
  setCameraPublishing: (device: { id: string; name: string } | null) => void;
  setCameraStarting: (starting: boolean) => void;
  setCameraStats: (stats: PublisherStats | null) => void;
  addTrack: (id: PublicationId, kind: 'video' | 'audio') => void;
  removeTrack: (id: PublicationId, kind: 'video' | 'audio') => void;
  setVolume: (id: PublicationId, volume: number) => void;
  setQuality: (id: PublicationId, quality: QualityChoice) => void;
  setSubscribed: (id: PublicationId, subscribed: boolean) => void;
  /** Quem transmitia parou ou saiu: o ladrilho vai junto, tendo sido assinado ou não. */
  dropPublication: (id: PublicationId) => void;
  focus: (id: PublicationId | null) => void;
  setSolo: (solo: boolean) => void;
  setDetached: (id: PublicationId | null) => void;
  setViewerIds: (ids: string[]) => void;
  setStats: (stats: PublisherStats | null) => void;
  reset: () => void;
}

const NO_CAMERA: CameraShare = {
  publishing: false,
  starting: false,
  deviceId: null,
  deviceName: null,
  stats: null,
};

const INITIAL: MediaState = {
  connection: 'idle',
  fault: null,
  publishing: false,
  sharingTitle: null,
  starting: false,
  sharingAudio: false,
  audioMode: null,
  publishPreset: '1080p60',
  shareAudio: true,
  camera: NO_CAMERA,
  publications: {},
  publicationOrder: [],
  focused: null,
  solo: false,
  detached: null,
  viewerIds: [],
  stats: null,
};

function blank(id: PublicationId): PublicationState {
  return {
    id,
    ownerId: ownerOfPublication(id),
    source: sourceOfPublication(id),
    hasVideo: false,
    hasAudio: false,
    volume: DEFAULT_VOLUME,
    quality: 'auto',
    subscribed: true,
  };
}

/**
 * Video and audio of one publication arrive as two separate tracks and in no
 * guaranteed order, so it is created by whichever lands first and only
 * disappears when both are gone.
 */
export function withTrack(
  state: MediaState,
  id: PublicationId,
  kind: 'video' | 'audio',
): MediaState {
  const existing = state.publications[id] ?? blank(id);
  const updated: PublicationState = {
    ...existing,
    hasVideo: kind === 'video' ? true : existing.hasVideo,
    hasAudio: kind === 'audio' ? true : existing.hasAudio,
  };
  if (
    existing.hasVideo === updated.hasVideo &&
    existing.hasAudio === updated.hasAudio &&
    state.publications[id] !== undefined
  ) {
    return state;
  }
  const known = state.publicationOrder.includes(id);
  return {
    ...state,
    publications: { ...state.publications, [id]: updated },
    publicationOrder: known ? state.publicationOrder : [...state.publicationOrder, id],
  };
}

export function withoutTrack(
  state: MediaState,
  id: PublicationId,
  kind: 'video' | 'audio',
): MediaState {
  const existing = state.publications[id];
  if (existing === undefined) {
    return state;
  }
  const updated: PublicationState = {
    ...existing,
    hasVideo: kind === 'video' ? false : existing.hasVideo,
    hasAudio: kind === 'audio' ? false : existing.hasAudio,
  };
  if (updated.hasVideo || updated.hasAudio || !updated.subscribed) {
    return { ...state, publications: { ...state.publications, [id]: updated } };
  }
  // Nada mais chegando desta publicacao: o ladrilho sai, e o foco e o destaque
  // saem com ele — apontar para uma tela que nao existe deixa a interface em
  // branco.
  const publications = { ...state.publications };
  delete publications[id];
  return {
    ...state,
    publications,
    publicationOrder: state.publicationOrder.filter((existing) => existing !== id),
    focused: state.focused === id ? null : state.focused,
    detached: state.detached === id ? null : state.detached,
  };
}

export const useMediaStore = create<MediaStore>()((set) => ({
  ...INITIAL,
  setConnection: (connection, fault = null) => {
    // A falha morre com o estado: qualquer transição que não seja `failed`
    // significa que a sala voltou, e um aviso que sobrevive ao próprio motivo é
    // pior do que aviso nenhum.
    set({ connection, fault: connection === 'failed' ? fault : null });
  },
  setPublishing: (publishing, sharingAudio, audioMode = null, title = null) => {
    set(
      publishing
        ? { publishing, sharingAudio, audioMode, starting: false, sharingTitle: title }
        : {
            publishing,
            sharingAudio,
            audioMode: null,
            starting: false,
            stats: null,
            sharingTitle: null,
            // O ladrilho do preview some junto: deixar o foco apontando para ele
            // deixaria a tela preta com um cromo em cima de nada.
            focused: null,
          },
    );
  },
  setStarting: (starting) => {
    set({ starting });
  },
  setShareAudio: (shareAudio) => {
    set({ shareAudio });
  },

  setPublishPreset: (publishPreset) => {
    set({ publishPreset });
  },
  setCameraPublishing: (device) => {
    set((state) =>
      device === null
        ? {
            camera: NO_CAMERA,
            // O ladrilho do preview da câmera some junto; o foco nele deixaria a
            // janela olhando para nada.
            focused: state.focused === selfPublication('camera') ? null : state.focused,
          }
        : {
            camera: {
              publishing: true,
              starting: false,
              deviceId: device.id,
              deviceName: device.name,
              stats: state.camera.stats,
            },
          },
    );
  },
  setCameraStarting: (starting) => {
    set((state) => ({ camera: { ...state.camera, starting } }));
  },
  setCameraStats: (stats) => {
    set((state) => ({ camera: { ...state.camera, stats } }));
  },
  addTrack: (id, kind) => {
    set((state) => withTrack(state, id, kind));
  },
  removeTrack: (id, kind) => {
    set((state) => withoutTrack(state, id, kind));
  },
  setVolume: (id, volume) => {
    set((state) => {
      const publication = state.publications[id];
      if (publication === undefined) {
        return state;
      }
      const clamped = Math.min(1, Math.max(0, volume));
      return {
        publications: { ...state.publications, [id]: { ...publication, volume: clamped } },
      };
    });
  },
  setQuality: (id, quality) => {
    set((state) => {
      const publication = state.publications[id];
      if (publication === undefined) {
        return state;
      }
      return { publications: { ...state.publications, [id]: { ...publication, quality } } };
    });
  },
  setSubscribed: (id, subscribed) => {
    set((state) => {
      const publication = state.publications[id];
      if (publication === undefined) {
        return state;
      }
      return { publications: { ...state.publications, [id]: { ...publication, subscribed } } };
    });
  },

  dropPublication: (id) => {
    set((state) => {
      if (state.publications[id] === undefined) {
        return state;
      }
      const publications = { ...state.publications };
      delete publications[id];
      return {
        publications,
        publicationOrder: state.publicationOrder.filter((existing) => existing !== id),
        focused: state.focused === id ? null : state.focused,
        detached: state.detached === id ? null : state.detached,
      };
    });
  },

  focus: (focused) => {
    // Voltar para a grade zera o exclusivo: a grade é, por definição, ver todas
    // as telas assinadas, e guardar o exclusivo faria o próximo foco esconder
    // as outras sem ninguém ter pedido.
    set(focused === null ? { focused, solo: false } : { focused });
  },

  setSolo: (solo) => {
    set({ solo });
  },
  setDetached: (detached) => {
    set({ detached });
  },
  setViewerIds: (viewerIds) => {
    set({ viewerIds });
  },
  setStats: (stats) => {
    set({ stats });
  },
  reset: () => {
    set(INITIAL);
  },
}));

/**
 * The suffix the server gives the publishing connection (ADR-0027).
 *
 * A person who shares is in the LiveKit room twice, and the screens store is
 * keyed by the person: the room's participant list, the owner shown on a tile
 * and the elapsed clock all come from the server under the plain user id.
 */
export const PUBLISHER_SUFFIX = '~pub';

export function ownerOf(identity: string): string {
  return identity.endsWith(PUBLISHER_SUFFIX)
    ? identity.slice(0, -PUBLISHER_SUFFIX.length)
    : identity;
}

/**
 * ADR-0028: while transmitting audio, other people's screen audio is silenced
 * locally.
 *
 * `EXCLUDE_TARGET_PROCESS_TREE` takes one process id and it is spent on Discord,
 * so our own playback is inside our own capture. Without this, two people
 * sharing audio at once would hear each other echoed, and everyone else would
 * receive one of them twice.
 *
 * Compartilhar uma **janela** é o caso em que nada disso acontece: o core
 * captura só a árvore daquele processo, o som das outras telas não entra, e
 * silenciá-lo seria tirar do usuário um áudio sem motivo nenhum (issue #11).
 */
export function shouldSilenceOtherScreens(state: MediaState): boolean {
  return state.publishing && state.sharingAudio && state.audioMode !== 'only_window';
}

/**
 * O dono dos ladrilhos de preview local (ADR-0030).
 *
 * Não é uma identidade do LiveKit e nunca chega ao servidor: a própria
 * publicação não é assinada, e o preview sai da captura local. O prefixo `~`
 * segue o mesmo formato do sufixo do publicador e garante que ela jamais colida
 * com um id de usuário do Discord, que é só dígitos.
 */
export const SELF_OWNER = '~self';

/** O ladrilho local de uma das nossas fontes: `~self:screen` ou `~self:camera`. */
export function selfPublication(source: PublicationSource): PublicationId {
  return publicationId(SELF_OWNER, source);
}

export function isSelfPublication(id: PublicationId): boolean {
  return ownerOfPublication(id) === SELF_OWNER;
}

/**
 * A ordem dos ladrilhos na grade, incluindo os da própria transmissão.
 *
 * O que é nosso entra **por último**: entrando na frente, começar a transmitir
 * empurraria as telas dos outros de lugar no meio de uma sessão, e mudança de
 * posição sem motivo é a coisa que mais parece defeito numa grade de vídeo.
 * Entre os nossos, a mesma ordem de sempre: tela e depois câmera.
 *
 * Quem foi deixado de fora não entra (ADR-0036): uma tela que não está sendo
 * assistida ocupando uma célula da grade — ou um lugar na coluna lateral do
 * foco — é espaço gasto para dizer que ali não há nada. O caminho de volta é a
 * lista de pessoas, que mostra a sala inteira, assistida ou não.
 */
export function visibleTiles(
  order: readonly PublicationId[],
  publications: Record<PublicationId, PublicationState>,
  mine: { screen: boolean; camera: boolean },
  showSelfPreview: boolean,
): PublicationId[] {
  const watched = order.filter((id) => publications[id]?.subscribed !== false);
  if (!showSelfPreview) {
    return watched;
  }
  const own = SOURCE_ORDER.filter((source) => mine[source]).map(selfPublication);
  return [...watched, ...own];
}

/** Publicações no ar que este espectador deixou de assistir, na ordem de chegada. */
export function leftPublicationIds(
  state: Pick<MediaState, 'publicationOrder' | 'publications'>,
): PublicationId[] {
  return state.publicationOrder.filter((id) => state.publications[id]?.subscribed === false);
}
