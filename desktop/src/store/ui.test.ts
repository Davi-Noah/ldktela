import { describe, expect, it, vi } from 'vitest';
import { useUiStore } from './ui';

function fresh() {
  useUiStore.setState({ toasts: [], chromeHolds: 0, showSelfPreview: true });
  return useUiStore.getState();
}

describe('avisos', () => {
  it('não repete a mesma mensagem duas vezes', () => {
    // Uma queda de mídia chega por mais de um caminho — o evento do core e o
    // desligamento da sala — e empilhar o mesmo texto duas vezes parece defeito.
    const store = fresh();
    store.toast('danger', 'A sala já está cheia.');
    store.toast('danger', 'A sala já está cheia.');
    expect(useUiStore.getState().toasts).toHaveLength(1);
  });

  it('some sozinho depois do tempo de leitura', () => {
    vi.useFakeTimers();
    try {
      fresh().toast('info', 'Pronto.');
      expect(useUiStore.getState().toasts).toHaveLength(1);
      vi.advanceTimersByTime(10_000);
      expect(useUiStore.getState().toasts).toHaveLength(0);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('travamento do cromo', () => {
  it('só libera quando o último motivo sai', () => {
    // Ponteiro sobre a barra e menu aberto são dois motivos que se sobrepõem:
    // com um booleano, fechar o menu esconderia a barra sob o cursor.
    const store = fresh();
    const pointer = store.holdChrome();
    const menu = store.holdChrome();
    expect(useUiStore.getState().chromeHolds).toBe(2);

    menu();
    expect(useUiStore.getState().chromeHolds).toBe(1);
    pointer();
    expect(useUiStore.getState().chromeHolds).toBe(0);
  });

  it('soltar duas vezes não deixa o contador negativo', () => {
    const release = fresh().holdChrome();
    release();
    release();
    expect(useUiStore.getState().chromeHolds).toBe(0);
  });
});

describe('a largura da lateral do foco parcial (issue #7)', () => {
  it('não deixa a lateral sumir nem engolir a tela em foco', () => {
    const store = useUiStore.getState();
    store.setRailWidth(0);
    expect(useUiStore.getState().railWidth).toBe(10);

    store.setRailWidth(90);
    expect(useUiStore.getState().railWidth).toBe(45);

    store.setRailWidth(33.4);
    expect(useUiStore.getState().railWidth).toBe(33);
  });
});
