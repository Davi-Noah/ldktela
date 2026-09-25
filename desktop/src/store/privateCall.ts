import { create } from 'zustand';
import type { PrivateCallState } from '../api/types/PrivateCallState';

interface PrivateCallStore {
  call: PrivateCallState | null;
  inviteCode: string | null;
  busy: boolean;
  error: string | null;
  opened: (call: PrivateCallState, inviteCode?: string) => void;
  closed: () => void;
  setBusy: (busy: boolean) => void;
  setError: (error: string | null) => void;
}

export const usePrivateCallStore = create<PrivateCallStore>()((set) => ({
  call: null,
  inviteCode: null,
  busy: false,
  error: null,
  opened: (call, inviteCode) => {
    set({ call, inviteCode: inviteCode ?? null, busy: false, error: null });
  },
  closed: () => {
    set({ call: null, inviteCode: null, busy: false, error: null });
  },
  setBusy: (busy) => {
    set({ busy });
  },
  setError: (error) => {
    set({ error });
  },
}));
