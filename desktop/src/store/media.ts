import { create } from 'zustand';
import type { AudioMode } from '../media/native';

export type MediaConnection = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'failed';

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
  /** True from the moment our screen track is published until it is dropped. */
  publishing: boolean;
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
  error: string | null;
}

interface MediaStore extends MediaState {
  setConnection: (connection: MediaConnection) => void;
  setPublishing: (publishing: boolean, sharingAudio: boolean, audioMode?: AudioMode | null) => void;
  setStarting: (starting: boolean) => void;
  setPublishPreset: (preset: PublishPreset) => void;
  addScreen: (identity: string, kind: 'video' | 'audio') => void;
  removeScreen: (identity: string, kind: 'video' | 'audio') => void;
  setVolume: (identity: string, volume: number) => void;
  setQuality: (identity: string, quality: QualityChoice) => void;
  focus: (identity: string | null) => void;
  setDetached: (identity: string | null) => void;
  setViewerIds: (ids: string[]) => void;
  setStats: (stats: PublisherStats | null) => void;
  setError: (error: string | null) => void;
  reset: () => void;
}

const INITIAL: MediaState = {
  connection: 'idle',
  publishing: false,
  starting: false,
  sharingAudio: false,
  audioMode: null,
  publishPreset: '1080p60',
  screens: {},
  screenOrder: [],
  focused: null,
  detached: null,
  viewerIds: [],
  stats: null,
  error: null,
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
  setConnection: (connection) => {
    set({ connection });
  },
  setPublishing: (publishing, sharingAudio, audioMode = null) => {
    set(
      publishing
        ? { publishing, sharingAudio, audioMode, starting: false }
        : { publishing, sharingAudio, audioMode: null, starting: false, stats: null },
    );
  },
  setStarting: (starting) => {
    set({ starting });
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
  setError: (error) => {
    set({ error });
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
 */
export function shouldSilenceOtherScreens(state: MediaState): boolean {
  return state.publishing && state.sharingAudio;
}
