import { create } from 'zustand';
import type { ReadNotes } from '../features/update/notes';

/**
 * As novidades à espera de serem lidas (ADR-0040).
 *
 * Em memória, como todo estado de interface (CLAUDE.md §2.8). O que precisa
 * sobreviver entre aberturas — qual versão já foi vista — mora no core.
 */
interface ReleaseNotesState {
  pending: { version: string; notes: ReadNotes } | null;
  show: (version: string, notes: ReadNotes) => void;
  clear: () => void;
}

export const useReleaseNotesStore = create<ReleaseNotesState>()((set) => ({
  pending: null,
  show: (version, notes) => {
    set({ pending: { version, notes } });
  },
  clear: () => {
    set({ pending: null });
  },
}));
