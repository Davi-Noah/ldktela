import { create } from 'zustand';

export type MediaConnection = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'failed';

/** Automatic follows the element size; the other two pin a simulcast layer. */
export type QualityChoice = 'auto' | 'high' | 'low';

export interface PublisherStats {
  bitrateKbps: number;
  fps: number;
  width: number;
  height: number;
}

interface MediaState {
  connection: MediaConnection;
  /** True from the moment our screen track is published until it is dropped. */
  publishing: boolean;
  /** True while the OS picker is open, so the button can say so. */
  starting: boolean;
  sharingAudio: boolean;
  /** Identity of the participant whose screen is on the video element, if any. */
  watching: string | null;
  /** Identities connected to the media room, minus ourselves: the viewers. */
  viewerIds: string[];
  quality: QualityChoice;
  stats: PublisherStats | null;
  error: string | null;
}

interface MediaStore extends MediaState {
  setConnection: (connection: MediaConnection) => void;
  setPublishing: (publishing: boolean, sharingAudio: boolean) => void;
  setStarting: (starting: boolean) => void;
  setWatching: (identity: string | null) => void;
  setViewerIds: (ids: string[]) => void;
  setQuality: (quality: QualityChoice) => void;
  setStats: (stats: PublisherStats | null) => void;
  setError: (error: string | null) => void;
  reset: () => void;
}

const INITIAL: MediaState = {
  connection: 'idle',
  publishing: false,
  starting: false,
  sharingAudio: false,
  watching: null,
  viewerIds: [],
  quality: 'auto',
  stats: null,
  error: null,
};

export const useMediaStore = create<MediaStore>()((set) => ({
  ...INITIAL,
  setConnection: (connection) => {
    set({ connection });
  },
  setPublishing: (publishing, sharingAudio) => {
    set(
      publishing
        ? { publishing, sharingAudio, starting: false }
        : { publishing, sharingAudio, starting: false, stats: null },
    );
  },
  setStarting: (starting) => {
    set({ starting });
  },
  setWatching: (watching) => {
    set({ watching });
  },
  setViewerIds: (viewerIds) => {
    set({ viewerIds });
  },
  setQuality: (quality) => {
    set({ quality });
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
