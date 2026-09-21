import { create } from 'zustand';
import type { AudioMode } from '../media/native';

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

/** One screen being received, keyed by the publisher's LiveKit identity. */
export interface ScreenState {
  identity: string;
  hasVideo: boolean;
  hasAudio: boolean;
  /** 0 to 1. Independent per screen (RF-35) and kept across focus changes. */
  volume: number;
  quality: QualityChoice;
}

const DEFAULT_VOLUME = 1;

interface MediaState {
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
  /** Screens being received, by publisher identity (RF-31). */
  screens: Record<string, ScreenState>;
  /** Arrival order, so the grid does not reshuffle on every render. */
  screenOrder: string[];
  /** Identity shown large. `null` means the grid. */
  focused: string | null;
  /** Identity currently in the picture-in-picture window (RF-33). */
  detached: string | null;
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
  addScreen: (identity: string, kind: 'video' | 'audio') => void;
  removeScreen: (identity: string, kind: 'video' | 'audio') => void;
  setVolume: (identity: string, volume: number) => void;
  setQuality: (identity: string, quality: QualityChoice) => void;
  focus: (identity: string | null) => void;
  setDetached: (identity: string | null) => void;
  setViewerIds: (ids: string[]) => void;
  setStats: (stats: PublisherStats | null) => void;
  reset: () => void;
}

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
  screens: {},
  screenOrder: [],
  focused: null,
  detached: null,
  viewerIds: [],
  stats: null,
};

function blank(identity: string): ScreenState {
  return {
    identity,
    hasVideo: false,
    hasAudio: false,
    volume: DEFAULT_VOLUME,
    quality: 'auto',
  };
}

/**
 * Video and audio of one screen arrive as two separate tracks and in no
 * guaranteed order, so the screen is created by whichever lands first and only
 * disappears when both are gone.
 */
export function withTrack(
  state: MediaState,
  identity: string,
  kind: 'video' | 'audio',
): MediaState {
  const existing = state.screens[identity] ?? blank(identity);
  const updated: ScreenState = {
    ...existing,
    hasVideo: kind === 'video' ? true : existing.hasVideo,
    hasAudio: kind === 'audio' ? true : existing.hasAudio,
  };
  if (
    existing.hasVideo === updated.hasVideo &&
    existing.hasAudio === updated.hasAudio &&
    state.screens[identity] !== undefined
  ) {
    return state;
  }
  const known = state.screenOrder.includes(identity);
  return {
    ...state,
    screens: { ...state.screens, [identity]: updated },
    screenOrder: known ? state.screenOrder : [...state.screenOrder, identity],
  };
}

export function withoutTrack(
  state: MediaState,
  identity: string,
  kind: 'video' | 'audio',
): MediaState {
  const existing = state.screens[identity];
  if (existing === undefined) {
    return state;
  }
  const updated: ScreenState = {
    ...existing,
    hasVideo: kind === 'video' ? false : existing.hasVideo,
    hasAudio: kind === 'audio' ? false : existing.hasAudio,
  };
  if (updated.hasVideo || updated.hasAudio) {
    return { ...state, screens: { ...state.screens, [identity]: updated } };
  }
  // Nada mais chegando desta pessoa: a tela sai, e o foco e o destaque saem com
  // ela — apontar para uma tela que nao existe deixa a interface em branco.
  const screens = { ...state.screens };
  delete screens[identity];
  return {
    ...state,
    screens,
    screenOrder: state.screenOrder.filter((id) => id !== identity),
    focused: state.focused === identity ? null : state.focused,
    detached: state.detached === identity ? null : state.detached,
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
  addScreen: (identity, kind) => {
    set((state) => withTrack(state, identity, kind));
  },
  removeScreen: (identity, kind) => {
    set((state) => withoutTrack(state, identity, kind));
  },
  setVolume: (identity, volume) => {
    set((state) => {
      const screen = state.screens[identity];
      if (screen === undefined) {
        return state;
      }
      const clamped = Math.min(1, Math.max(0, volume));
      return { screens: { ...state.screens, [identity]: { ...screen, volume: clamped } } };
    });
  },
  setQuality: (identity, quality) => {
    set((state) => {
      const screen = state.screens[identity];
      if (screen === undefined) {
        return state;
      }
      return { screens: { ...state.screens, [identity]: { ...screen, quality } } };
    });
  },
  focus: (focused) => {
    set({ focused });
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
 * A identidade do ladrilho da própria tela (ADR-0030).
 *
 * Não é uma identidade do LiveKit e nunca chega ao servidor: a própria
 * publicação não é assinada, e o preview sai da captura local. O prefixo `~`
 * segue o mesmo formato do sufixo do publicador e garante que ela jamais colida
 * com um id de usuário do Discord, que é só dígitos.
 */
export const SELF_ID = '~self';

/**
 * A ordem dos ladrilhos na grade, incluindo o da própria tela.
 *
 * A própria tela entra **por último**: entrando na frente, começar a transmitir
 * empurraria as telas dos outros de lugar no meio de uma sessão, e mudança de
 * posição sem motivo é a coisa que mais parece defeito numa grade de vídeo.
 */
export function visibleTiles(
  screenOrder: readonly string[],
  publishing: boolean,
  showSelfPreview: boolean,
): string[] {
  return publishing && showSelfPreview ? [...screenOrder, SELF_ID] : [...screenOrder];
}
