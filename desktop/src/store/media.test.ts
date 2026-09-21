import { describe, expect, it } from 'vitest';
import {
  ownerOf,
  SELF_ID,
  shouldSilenceOtherScreens,
  useMediaStore,
  visibleTiles,
  withoutTrack,
  withTrack,
} from './media';

function fresh() {
  useMediaStore.getState().reset();
  return useMediaStore.getState();
}

describe('screens', () => {
  it('creates a screen from whichever track lands first', () => {
    // Vídeo e áudio chegam como duas trilhas separadas e sem ordem garantida.
    const audioFirst = withTrack(fresh(), 'ana', 'audio');
    expect(audioFirst.screens.ana?.hasAudio).toBe(true);
    expect(audioFirst.screens.ana?.hasVideo).toBe(false);

    const both = withTrack(audioFirst, 'ana', 'video');
    expect(both.screens.ana?.hasVideo).toBe(true);
    expect(both.screenOrder).toEqual(['ana']);
  });

  it('keeps the screen while any track remains', () => {
    let state = withTrack(fresh(), 'ana', 'video');
    state = withTrack(state, 'ana', 'audio');
    state = withoutTrack(state, 'ana', 'audio');
    expect(state.screens.ana).toBeDefined();
    expect(state.screens.ana?.hasVideo).toBe(true);
  });

  it('drops the screen only when the last track goes', () => {
    let state = withTrack(fresh(), 'ana', 'video');
    state = withoutTrack(state, 'ana', 'video');
    expect(state.screens.ana).toBeUndefined();
    expect(state.screenOrder).toEqual([]);
  });

  it('clears focus and detach when that screen disappears', () => {
    // Apontar para uma tela que não existe mais deixaria a interface em branco.
    let state = withTrack(fresh(), 'ana', 'video');
    state = { ...state, focused: 'ana', detached: 'ana' };
    state = withoutTrack(state, 'ana', 'video');
    expect(state.focused).toBeNull();
    expect(state.detached).toBeNull();
  });

  it('leaves focus alone when a different screen disappears', () => {
    let state = withTrack(fresh(), 'ana', 'video');
    state = withTrack(state, 'bia', 'video');
    state = { ...state, focused: 'ana' };
    state = withoutTrack(state, 'bia', 'video');
    expect(state.focused).toBe('ana');
  });

  it('preserves arrival order so the grid does not reshuffle', () => {
    let state = withTrack(fresh(), 'ana', 'video');
    state = withTrack(state, 'bia', 'video');
    state = withTrack(state, 'ana', 'audio');
    expect(state.screenOrder).toEqual(['ana', 'bia']);
  });

  it('returns the same object when nothing changed', () => {
    const state = withTrack(fresh(), 'ana', 'video');
    expect(withTrack(state, 'ana', 'video')).toBe(state);
    expect(withoutTrack(state, 'quem-nao-existe', 'video')).toBe(state);
  });
});

describe('volume', () => {
  it('is independent per screen and survives a focus change', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.addScreen('ana', 'audio');
    store.addScreen('bia', 'audio');
    store.setVolume('ana', 0.25);
    store.focus('bia');

    expect(useMediaStore.getState().screens.ana?.volume).toBe(0.25);
    expect(useMediaStore.getState().screens.bia?.volume).toBe(1);
  });

  it('clamps out-of-range values instead of trusting the input', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.addScreen('ana', 'audio');
    store.setVolume('ana', 5);
    expect(useMediaStore.getState().screens.ana?.volume).toBe(1);
    store.setVolume('ana', -3);
    expect(useMediaStore.getState().screens.ana?.volume).toBe(0);
  });

  it('ignores a screen that is not there', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.setVolume('fantasma', 0.5);
    expect(useMediaStore.getState().screens.fantasma).toBeUndefined();
  });
});

describe('a identidade de quem publica (ADR-0027)', () => {
  it('resolve as duas conexões de uma pessoa para o mesmo dono', () => {
    // Quem compartilha está na sala duas vezes; a grade é indexada pela pessoa,
    // e é a pessoa que a lista de participantes do servidor conhece.
    expect(ownerOf('0198c0de-0000-7000-8000-000000000001~pub')).toBe(
      '0198c0de-0000-7000-8000-000000000001',
    );
    expect(ownerOf('0198c0de-0000-7000-8000-000000000001')).toBe(
      '0198c0de-0000-7000-8000-000000000001',
    );
  });

  it('não corta um sufixo que está no meio', () => {
    expect(ownerOf('ana~pubxyz')).toBe('ana~pubxyz');
  });
});

describe('silenciar as telas alheias ao transmitir áudio (ADR-0028)', () => {
  it('silencia só enquanto transmitimos com áudio', () => {
    // A exclusão do WASAPI aceita um processo só, e ele é gasto no Discord:
    // o nosso próprio áudio fica dentro da nossa captura.
    const store = useMediaStore.getState();
    store.reset();
    expect(shouldSilenceOtherScreens(useMediaStore.getState())).toBe(false);

    store.setPublishing(true, false);
    expect(shouldSilenceOtherScreens(useMediaStore.getState())).toBe(false);

    store.setPublishing(true, true, 'excluding_discord');
    expect(shouldSilenceOtherScreens(useMediaStore.getState())).toBe(true);

    store.setPublishing(false, false);
    expect(shouldSilenceOtherScreens(useMediaStore.getState())).toBe(false);
  });

  it('não silencia quando só a janela compartilhada está sendo capturada (issue #11)', () => {
    // Capturando apenas a árvore do processo daquela janela, o som das telas
    // alheias não entra na captura — silenciá-lo seria tirar do usuário um
    // áudio que nada obriga a tirar.
    const store = useMediaStore.getState();
    store.reset();
    store.setPublishing(true, true, 'only_window');
    expect(shouldSilenceOtherScreens(useMediaStore.getState())).toBe(false);
  });

  it('esquece o modo de áudio ao parar', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.setPublishing(true, true, 'whole_system');
    expect(useMediaStore.getState().audioMode).toBe('whole_system');
    store.setPublishing(false, false);
    expect(useMediaStore.getState().audioMode).toBeNull();
  });
});

describe('ladrilho da própria tela (ADR-0030)', () => {
  it('entra por último, para não empurrar as telas dos outros de lugar', () => {
    expect(visibleTiles(['ana', 'bruno'], true, true)).toEqual(['ana', 'bruno', SELF_ID]);
  });

  it('não aparece quando não se está transmitindo', () => {
    expect(visibleTiles(['ana'], false, true)).toEqual(['ana']);
  });

  it('some quando o usuário escolhe não se ver', () => {
    expect(visibleTiles(['ana'], true, false)).toEqual(['ana']);
  });

  it('pode ser o único ladrilho da grade', () => {
    // Transmitindo sozinho, a sala deixa de ser uma lista de participantes e
    // passa a ser a própria tela — que é o que o Discord mostra.
    expect(visibleTiles([], true, true)).toEqual([SELF_ID]);
  });

  it('nunca colide com um id do Discord, que é só dígitos', () => {
    expect(SELF_ID).not.toMatch(/^\d+$/);
  });
});

describe('parar de transmitir', () => {
  it('solta o foco, para não sobrar cromo em cima de nada', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.setPublishing(true, false, null, 'Tela 1');
    store.focus(SELF_ID);
    expect(useMediaStore.getState().focused).toBe(SELF_ID);

    store.setPublishing(false, false);
    expect(useMediaStore.getState().focused).toBeNull();
  });

  it('esquece o que estava sendo compartilhado', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.setPublishing(true, false, null, 'Elden Ring');
    expect(useMediaStore.getState().sharingTitle).toBe('Elden Ring');
    store.setPublishing(false, false);
    expect(useMediaStore.getState().sharingTitle).toBeNull();
  });
});

/**
 * O motivo da falha existe para virar texto na tela, e some com ela.
 *
 * Sem isto, `failed` produzia uma sala de aparência normal sobre uma conexão que
 * não existe — nome do canal, lista de participantes, botão de compartilhar — e
 * a única pista era uma torrada que já tinha sumido.
 */
describe('o motivo pelo qual a conexão de mídia desistiu', () => {
  it('acompanha o estado de falha e é limpo quando a sala volta', () => {
    const store = useMediaStore.getState();

    store.setConnection('failed', 'duplicate_identity');
    expect(useMediaStore.getState().fault).toBe('duplicate_identity');

    store.setConnection('connecting');
    expect(useMediaStore.getState().fault).toBeNull();

    store.setConnection('failed', 'unreachable');
    expect(useMediaStore.getState().fault).toBe('unreachable');

    store.setConnection('connected');
    expect(useMediaStore.getState().fault).toBeNull();
  });

  it('não sobrevive a um motivo que ninguém informou', () => {
    useMediaStore.getState().setConnection('failed', 'duplicate_identity');
    useMediaStore.getState().setConnection('failed');
    expect(useMediaStore.getState().fault).toBeNull();
  });
});

describe('o som do computador (issue #8)', () => {
  it('vem ligado, porque é o que o usuário espera de compartilhar tela', () => {
    expect(fresh().shareAudio).toBe(true);
  });

  it('lembra que foi desligado, para não voltar sozinho na próxima transmissão', () => {
    // Religar sozinho é pior do que começar desligado: quem tirou o áudio tinha
    // um motivo, e descobriria pelo amigo do outro lado que ele voltou.
    useMediaStore.getState().setShareAudio(false);
    expect(useMediaStore.getState().shareAudio).toBe(false);
  });
});
