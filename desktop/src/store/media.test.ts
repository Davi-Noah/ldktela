import { describe, expect, it } from 'vitest';
import {
  leftPublicationIds,
  type MediaState,
  ownerOf,
  selfPublication,
  SELF_OWNER,
  shouldSilenceOtherScreens,
  useMediaStore,
  visibleTiles,
  withoutTrack,
  withTrack,
} from './media';
import { publicationId } from '../media/publication';

/** As publicações de tela de quem aparece nos testes, por extenso. */
const ANA = publicationId('ana', 'screen');
const ANA_CAM = publicationId('ana', 'camera');
const BIA = publicationId('bia', 'screen');
const BRUNO = publicationId('bruno', 'screen');

/** Só a tela nossa, o caso de quase todo teste daqui. */
const MINE = { screen: true, camera: false };
const NOTHING_MINE = { screen: false, camera: false };

function fresh() {
  useMediaStore.getState().reset();
  return useMediaStore.getState();
}

function leftScreen(state: MediaState, id: string): MediaState {
  useMediaStore.setState(state);
  useMediaStore.getState().setSubscribed(id, false);
  return useMediaStore.getState();
}

function enteredScreen(state: MediaState, id: string): MediaState {
  useMediaStore.setState(state);
  useMediaStore.getState().setSubscribed(id, true);
  return useMediaStore.getState();
}

describe('screens', () => {
  it('creates a screen from whichever track lands first', () => {
    // Vídeo e áudio chegam como duas trilhas separadas e sem ordem garantida.
    const audioFirst = withTrack(fresh(), ANA, 'audio');
    expect(audioFirst.publications[ANA]?.hasAudio).toBe(true);
    expect(audioFirst.publications[ANA]?.hasVideo).toBe(false);

    const both = withTrack(audioFirst, ANA, 'video');
    expect(both.publications[ANA]?.hasVideo).toBe(true);
    expect(both.publicationOrder).toEqual([ANA]);
  });

  it('keeps the screen while any track remains', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withTrack(state, ANA, 'audio');
    state = withoutTrack(state, ANA, 'audio');
    expect(state.publications[ANA]).toBeDefined();
    expect(state.publications[ANA]?.hasVideo).toBe(true);
  });

  it('drops the screen only when the last track goes', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withoutTrack(state, ANA, 'video');
    expect(state.publications[ANA]).toBeUndefined();
    expect(state.publicationOrder).toEqual([]);
  });

  it('clears focus and detach when that screen disappears', () => {
    // Apontar para uma tela que não existe mais deixaria a interface em branco.
    let state = withTrack(fresh(), ANA, 'video');
    state = { ...state, focused: ANA, detached: ANA };
    state = withoutTrack(state, ANA, 'video');
    expect(state.focused).toBeNull();
    expect(state.detached).toBeNull();
  });

  it('leaves focus alone when a different screen disappears', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withTrack(state, BIA, 'video');
    state = { ...state, focused: ANA };
    state = withoutTrack(state, BIA, 'video');
    expect(state.focused).toBe(ANA);
  });

  it('preserves arrival order so the grid does not reshuffle', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withTrack(state, BIA, 'video');
    state = withTrack(state, ANA, 'audio');
    expect(state.publicationOrder).toEqual([ANA, BIA]);
  });

  it('returns the same object when nothing changed', () => {
    const state = withTrack(fresh(), ANA, 'video');
    expect(withTrack(state, ANA, 'video')).toBe(state);
    expect(withoutTrack(state, 'quem-nao-existe:screen', 'video')).toBe(state);
  });
});

describe('entrar e sair de uma tela (issue #6)', () => {
  it('mantém o ladrilho de quem eu deixei de ver, ou não haveria caminho de volta', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withTrack(state, ANA, 'audio');
    state = leftScreen(state, ANA);
    state = withoutTrack(state, ANA, 'video');
    state = withoutTrack(state, ANA, 'audio');

    expect(state.publications[ANA]?.subscribed).toBe(false);
    expect(state.publications[ANA]?.hasVideo).toBe(false);
    expect(state.publicationOrder).toEqual([ANA]);
  });

  it('tira o ladrilho quando quem transmitia parou, e não quando eu saí', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withoutTrack(state, ANA, 'video');
    expect(state.publications[ANA]).toBeUndefined();
  });

  it('começa assinada: sair é uma escolha, e não o padrão', () => {
    expect(withTrack(fresh(), ANA, 'video').publications[ANA]?.subscribed).toBe(true);
  });
});

describe('volume', () => {
  it('is independent per screen and survives a focus change', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.addTrack(ANA, 'audio');
    store.addTrack(BIA, 'audio');
    store.setVolume(ANA, 0.25);
    store.focus(BIA);

    expect(useMediaStore.getState().publications[ANA]?.volume).toBe(0.25);
    expect(useMediaStore.getState().publications[BIA]?.volume).toBe(1);
  });

  it('clamps out-of-range values instead of trusting the input', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.addTrack(ANA, 'audio');
    store.setVolume(ANA, 5);
    expect(useMediaStore.getState().publications[ANA]?.volume).toBe(1);
    store.setVolume(ANA, -3);
    expect(useMediaStore.getState().publications[ANA]?.volume).toBe(0);
  });

  it('ignores a screen that is not there', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.setVolume('fantasma', 0.5);
    expect(useMediaStore.getState().publications.fantasma).toBeUndefined();
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

describe('o layout mostra só o que se assiste (issue #7)', () => {
  it('não gasta espaço com a tela de quem eu deixei de assistir', () => {
    // O ladrilho vazio ocupava uma célula inteira da grade — e, no foco
    // parcial, um lugar na coluna lateral — para dizer "aqui não tem nada".
    let state = withTrack(withTrack(fresh(), ANA, 'video'), BRUNO, 'video');
    state = leftScreen(state, BRUNO);
    expect(visibleTiles(state.publicationOrder, state.publications, NOTHING_MINE, false)).toEqual([
      ANA,
    ]);
    expect(leftPublicationIds(state)).toEqual([BRUNO]);
  });

  it('devolve o ladrilho quando se entra de novo', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = leftScreen(state, ANA);
    state = enteredScreen(state, ANA);
    expect(visibleTiles(state.publicationOrder, state.publications, NOTHING_MINE, false)).toEqual([
      ANA,
    ]);
    expect(leftPublicationIds(state)).toEqual([]);
  });
});

describe('foco exclusivo (issue #7)', () => {
  it('sair do foco volta para a grade inteira, e não para uma tela sozinha', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.focus(ANA);
    store.setSolo(true);
    expect(useMediaStore.getState().solo).toBe(true);

    store.focus(null);
    expect(useMediaStore.getState().solo).toBe(false);
  });

  it('continua exclusivo ao trocar de tela em foco', () => {
    // Trocar de tela pelo seletor de foco é continuar vendo uma de cada vez;
    // devolver a lateral no meio disso seria desfazer a escolha sozinho.
    const store = useMediaStore.getState();
    store.reset();
    store.focus(ANA);
    store.setSolo(true);
    store.focus(BRUNO);
    expect(useMediaStore.getState().solo).toBe(true);
  });
});

describe('ladrilho da própria tela (ADR-0030)', () => {
  it('entra por último, para não empurrar as telas dos outros de lugar', () => {
    expect(visibleTiles([ANA, BRUNO], {}, MINE, true)).toEqual([
      ANA,
      BRUNO,
      selfPublication('screen'),
    ]);
  });

  it('não aparece quando não se está transmitindo', () => {
    expect(visibleTiles([ANA], {}, NOTHING_MINE, true)).toEqual([ANA]);
  });

  it('some quando o usuário escolhe não se ver', () => {
    expect(visibleTiles([ANA], {}, MINE, false)).toEqual([ANA]);
  });

  it('pode ser o único ladrilho da grade', () => {
    // Transmitindo sozinho, a sala deixa de ser uma lista de participantes e
    // passa a ser a própria tela — que é o que o Discord mostra.
    expect(visibleTiles([], {}, MINE, true)).toEqual([selfPublication('screen')]);
  });

  it('nunca colide com um id do Discord, que é só dígitos', () => {
    expect(SELF_OWNER).not.toMatch(/^\d+$/);
  });
});

describe('parar de transmitir', () => {
  it('solta o foco, para não sobrar cromo em cima de nada', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.setPublishing(true, false, null, 'Tela 1');
    store.focus(selfPublication('screen'));
    expect(useMediaStore.getState().focused).toBe(selfPublication('screen'));

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

/**
 * A câmera é uma publicação ao lado da tela (ADR-0038), e não um estado dela.
 * Cada teste aqui existe porque a alternativa — tratar a pessoa como unidade —
 * fazia uma fonte apagar a outra.
 */
describe('câmera ao lado da tela (ADR-0038)', () => {
  it('dá ladrilhos separados para a tela e a câmera da mesma pessoa', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withTrack(state, ANA_CAM, 'video');

    expect(state.publicationOrder).toEqual([ANA, ANA_CAM]);
    expect(state.publications[ANA]?.source).toBe('screen');
    expect(state.publications[ANA_CAM]?.source).toBe('camera');
    expect(state.publications[ANA_CAM]?.ownerId).toBe('ana');
  });

  it('sair da câmera de alguém deixa a tela da mesma pessoa no lugar', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withTrack(state, ANA_CAM, 'video');
    state = leftScreen(state, ANA_CAM);

    expect(state.publications[ANA]?.subscribed).toBe(true);
    expect(visibleTiles(state.publicationOrder, state.publications, NOTHING_MINE, false)).toEqual([
      ANA,
    ]);
  });

  it('o volume de uma não alcança a outra', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.addTrack(ANA, 'audio');
    store.addTrack(ANA_CAM, 'video');
    store.setVolume(ANA, 0.2);

    expect(useMediaStore.getState().publications[ANA]?.volume).toBe(0.2);
    expect(useMediaStore.getState().publications[ANA_CAM]?.volume).toBe(1);
  });

  it('a câmera que acaba não derruba a tela que continua', () => {
    let state = withTrack(fresh(), ANA, 'video');
    state = withTrack(state, ANA_CAM, 'video');
    state = withoutTrack(state, ANA_CAM, 'video');

    expect(state.publications[ANA_CAM]).toBeUndefined();
    expect(state.publications[ANA]?.hasVideo).toBe(true);
  });

  it('mostra os dois ladrilhos próprios, tela antes de câmera', () => {
    expect(visibleTiles([], {}, { screen: true, camera: true }, true)).toEqual([
      selfPublication('screen'),
      selfPublication('camera'),
    ]);
  });

  it('mostra só a câmera quando é só ela que está no ar', () => {
    expect(visibleTiles([], {}, { screen: false, camera: true }, true)).toEqual([
      selfPublication('camera'),
    ]);
  });

  it('parar a câmera solta o foco que estava nela, e não o da tela', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.setCameraPublishing({ id: 'cam-1', name: 'Logitech' });
    store.focus(selfPublication('camera'));
    store.setCameraPublishing(null);
    expect(useMediaStore.getState().focused).toBeNull();

    store.setPublishing(true, false, null, 'Tela 1');
    store.setCameraPublishing({ id: 'cam-1', name: 'Logitech' });
    store.focus(selfPublication('screen'));
    store.setCameraPublishing(null);
    expect(useMediaStore.getState().focused).toBe(selfPublication('screen'));
  });

  it('esquece o dispositivo ao desligar, para o painel não mentir', () => {
    const store = useMediaStore.getState();
    store.reset();
    store.setCameraPublishing({ id: 'cam-1', name: 'Logitech' });
    expect(useMediaStore.getState().camera.deviceName).toBe('Logitech');
    store.setCameraPublishing(null);
    expect(useMediaStore.getState().camera.publishing).toBe(false);
    expect(useMediaStore.getState().camera.deviceName).toBeNull();
  });

  it('a câmera não mexe no silenciamento do áudio, que é da tela (ADR-0028)', () => {
    // A câmera nunca carrega áudio, entao ligá-la não pode calar as telas
    // alheias nem impedir que elas sejam caladas.
    const store = useMediaStore.getState();
    store.reset();
    store.setCameraPublishing({ id: 'cam-1', name: 'Logitech' });
    expect(shouldSilenceOtherScreens(useMediaStore.getState())).toBe(false);

    store.setPublishing(true, true, 'excluding_discord');
    expect(shouldSilenceOtherScreens(useMediaStore.getState())).toBe(true);
  });
});
