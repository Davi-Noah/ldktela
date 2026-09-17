import { create } from 'zustand';
import type { Update } from '@tauri-apps/plugin-updater';
import type { AvailableUpdate } from '../platform/updater';

export type UpdaterStatus = 'idle' | 'available' | 'downloading' | 'error';

interface DownloadProgress {
  downloadedBytes: number;
  /** `null` when the server did not send a content length. */
  totalBytes: number | null;
}

interface UpdaterState {
  status: UpdaterStatus;
  update: Update | null;
  info: AvailableUpdate | null;
  progress: DownloadProgress | null;
  error: string | null;
  /** "Agora não": hides the banner without forgetting the update exists, so a
      later manual check does not have to hit the network again to remember. */
  dismissed: boolean;
}

interface UpdaterStore extends UpdaterState {
  available: (update: Update, info: AvailableUpdate) => void;
  startDownload: () => void;
  setProgress: (progress: DownloadProgress) => void;
  fail: (message: string) => void;
  dismiss: () => void;
  reset: () => void;
}

const INITIAL: UpdaterState = {
  status: 'idle',
  update: null,
  info: null,
  progress: null,
  error: null,
  dismissed: false,
};

export const useUpdaterStore = create<UpdaterStore>()((set) => ({
  ...INITIAL,
  available: (update, info) => {
    set({ status: 'available', update, info, error: null, dismissed: false });
  },
  startDownload: () => {
    set({ status: 'downloading', progress: { downloadedBytes: 0, totalBytes: null }, error: null });
  },
  setProgress: (progress) => {
    set({ progress });
  },
  fail: (error) => {
    set({ status: 'available', error });
  },
  dismiss: () => {
    set({ dismissed: true });
  },
  reset: () => {
    set(INITIAL);
  },
}));
