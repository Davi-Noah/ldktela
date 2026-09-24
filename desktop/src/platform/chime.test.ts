import { describe, expect, it } from 'vitest';
import { chimeForShare, shouldChime } from './chime';

/**
 * O interessante de um aviso sonoro é o silêncio dele: um sino que toca quando
 * não devia é o sino que a pessoa desliga — e aqui não há como desligar.
 */
describe('o aviso de uma tela que entra ou sai', () => {
  it('não toca para a própria transmissão', () => {
    expect(
      shouldChime({
        publisherId: 'ana',
        selfId: 'ana',
        now: 1000,
        lastAt: null,
        transmittingOwnPlayback: false,
      }),
    ).toBe(false);
  });

  it('toca para a tela de outra pessoa', () => {
    expect(
      shouldChime({
        publisherId: 'ana',
        selfId: 'bruno',
        now: 1000,
        lastAt: null,
        transmittingOwnPlayback: false,
      }),
    ).toBe(true);
  });

  it('toca mesmo sem saber quem somos, porque calar seria pior', () => {
    // O identificador da sessão chega um instante depois do READY; perder o
    // primeiro aviso da sessão por causa disso é pior do que o risco de anunciar
    // a própria tela uma vez.
    expect(
      shouldChime({
        publisherId: 'ana',
        selfId: undefined,
        now: 1000,
        lastAt: null,
        transmittingOwnPlayback: false,
      }),
    ).toBe(true);
  });

  it('colapsa uma rajada num aviso só', () => {
    // Retomar o gateway depois de uma queda entrega de uma vez os eventos
    // perdidos (docs/websocket.md §7). Sem isto, voltar tocaria uma sequência.
    expect(
      shouldChime({
        publisherId: 'ana',
        selfId: 'eu',
        now: 1200,
        lastAt: 1000,
        transmittingOwnPlayback: false,
      }),
    ).toBe(false);
  });

  it('volta a tocar quando a rajada passou', () => {
    expect(
      shouldChime({
        publisherId: 'ana',
        selfId: 'eu',
        now: 3000,
        lastAt: 1000,
        transmittingOwnPlayback: false,
      }),
    ).toBe(true);
  });

  it('não toca enquanto a própria captura grava este aplicativo', () => {
    // ADR-0028. O sino sai do WebView2 que a captura da tela inteira grava:
    // tocado agora, ele vai junto na transmissão, volta pelo áudio de quem
    // assiste e realimenta o laço — que foi o defeito relatado.
    expect(
      shouldChime({
        publisherId: 'ana',
        selfId: 'bruno',
        now: 1000,
        lastAt: null,
        transmittingOwnPlayback: true,
      }),
    ).toBe(false);
  });

  it('não quebra onde não existe áudio', () => {
    // jsdom não tem `AudioContext`, e nem todo runtime terá. O aplicativo
    // continua inteiro; o que falta é o som.
    expect(() => {
      chimeForShare('start', 'ana', 'eu', false);
      chimeForShare('stop', 'ana', 'eu', false);
    }).not.toThrow();
  });
});
