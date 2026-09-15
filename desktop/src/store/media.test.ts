import { describe, expect, it } from 'vitest';
import { useMediaStore, withoutTrack, withTrack } from './media';

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
