import { create } from 'zustand';

/**
 * Estado de interface, em memória (CLAUDE.md §2.8).
 *
 * Só o que não é domínio: avisos passageiros, se o cromo pode se esconder, e a
 * preferência de ver a própria tela. Nada daqui sobrevive ao fechamento do
 * aplicativo, e nada daqui vale a pena persistir sem um cofre de preferências
 * no core — que ainda não existe.
 */

export type ToastTone = 'info' | 'warning' | 'danger';

export interface Toast {
  id: number;
  tone: ToastTone;
  text: string;
}

/** Tempo de leitura de uma linha, com folga. Erro fica mais. */
const TOAST_MS: Record<ToastTone, number> = {
  info: 4000,
  warning: 7000,
  danger: 9000,
};

interface UiState {
  toasts: Toast[];
  /**
   * Quantos motivos existem agora para o cromo não se esconder: menu aberto,
   * ponteiro sobre a barra. Contador e não booleano porque os motivos se
   * sobrepõem, e o último a sair é que solta.
   */
  chromeHolds: number;
  /** RF-31 e ADR-0030: ver, ou não, a própria tela na grade. */
  showSelfPreview: boolean;
}

interface UiStore extends UiState {
  toast: (tone: ToastTone, text: string) => void;
  dismiss: (id: number) => void;
  holdChrome: () => () => void;
  setShowSelfPreview: (show: boolean) => void;
}

let nextId = 1;

export const useUiStore = create<UiStore>()((set, get) => ({
  toasts: [],
  chromeHolds: 0,
  showSelfPreview: true,

  toast: (tone, text) => {
    // Mesma mensagem duas vezes seguidas é uma mensagem, não duas: uma queda de
    // mídia dispara o mesmo aviso por vários caminhos.
    const current = get().toasts;
    if (current.some((item) => item.text === text)) {
      return;
    }
    const id = nextId;
    nextId += 1;
    set({ toasts: [...current, { id, tone, text }] });
    setTimeout(() => {
      get().dismiss(id);
    }, TOAST_MS[tone]);
  },

  dismiss: (id) => {
    set((state) => ({ toasts: state.toasts.filter((item) => item.id !== id) }));
  },

  holdChrome: () => {
    set((state) => ({ chromeHolds: state.chromeHolds + 1 }));
    let released = false;
    return () => {
      if (released) {
        return;
      }
      released = true;
      set((state) => ({ chromeHolds: Math.max(0, state.chromeHolds - 1) }));
    };
  },

  setShowSelfPreview: (showSelfPreview) => {
    set({ showSelfPreview });
  },
}));
