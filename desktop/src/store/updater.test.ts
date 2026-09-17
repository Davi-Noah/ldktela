import { describe, expect, it } from 'vitest';
import { useUpdaterStore } from './updater';

function fresh() {
  useUpdaterStore.getState().reset();
  return useUpdaterStore.getState();
}

describe('a store do atualizador', () => {
  it('some quando nada foi encontrado', () => {
    expect(fresh().status).toBe('idle');
  });

  it('marca uma atualização como disponível e não a esconde', () => {
    const store = fresh();
    const update = {} as never;
    store.available(update, { version: '0.2.0', notes: null });

    const state = useUpdaterStore.getState();
    expect(state.status).toBe('available');
    expect(state.info?.version).toBe('0.2.0');
    expect(state.dismissed).toBe(false);
  });

  it('"agora não" esconde a barra sem esquecer que existe atualização', () => {
    // O ponto de existir `dismissed` em vez de voltar a `idle`: uma checagem
    // periódica não deveria custar rede de novo só para redescobrir o que já
    // sabia.
    const store = fresh();
    store.available({} as never, { version: '0.2.0', notes: null });
    store.dismiss();

    const state = useUpdaterStore.getState();
    expect(state.dismissed).toBe(true);
    expect(state.info?.version).toBe('0.2.0');
  });

  it('uma falha ao instalar volta para "disponível", não para "idle"', () => {
    // Falhar não pode fazer a atualização sumir da tela — a pessoa perderia a
    // única forma de tentar de novo sem esperar a próxima checagem periódica.
    const store = fresh();
    store.available({} as never, { version: '0.2.0', notes: null });
    store.startDownload();
    store.fail('sem conexão');

    const state = useUpdaterStore.getState();
    expect(state.status).toBe('available');
    expect(state.error).toBe('sem conexão');
    expect(state.info?.version).toBe('0.2.0');
  });

  it('o progresso reflete o total ausente sem virar zero permanente', () => {
    const store = fresh();
    store.available({} as never, { version: '0.2.0', notes: null });
    store.startDownload();
    store.setProgress({ downloadedBytes: 4096, totalBytes: null });

    expect(useUpdaterStore.getState().progress).toEqual({
      downloadedBytes: 4096,
      totalBytes: null,
    });
  });
});
