import { create } from 'zustand';
import type { CurrentUser } from '../api/types/CurrentUser';
import type { GatewayStatus } from '../gateway/client';

export type AuthPhase = 'booting' | 'pairing' | 'authenticated' | 'update_required';

interface SessionStore {
  phase: AuthPhase;
  user: CurrentUser | null;
  gateway: GatewayStatus;
  /** Shown on the pairing screen. Always the same text for invalid and expired. */
  pairingError: string | null;
  pairing: boolean;
  setPhase: (phase: AuthPhase) => void;
  signedIn: (user: CurrentUser) => void;
  signedOut: () => void;
  setGateway: (status: GatewayStatus) => void;
  setPairingError: (message: string | null) => void;
  setPairing: (busy: boolean) => void;
}

export const useSessionStore = create<SessionStore>()((set) => ({
  phase: 'booting',
  user: null,
  gateway: 'idle',
  pairingError: null,
  pairing: false,
  setPhase: (phase) => {
    set({ phase });
  },
  signedIn: (user) => {
    set({ phase: 'authenticated', user, pairingError: null, pairing: false });
  },
  signedOut: () => {
    set({ phase: 'pairing', user: null, gateway: 'idle', pairing: false });
  },
  setGateway: (gateway) => {
    set({ gateway });
  },
  setPairingError: (pairingError) => {
    set({ pairingError });
  },
  setPairing: (pairing) => {
    set({ pairing });
  },
}));
