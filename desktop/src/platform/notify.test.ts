import { describe, expect, it } from 'vitest';
import { shouldNotify } from './notify';

/**
 * RF-27. O interessante aqui é o silêncio: o aplicativo passa o dia na bandeja,
 * então a notificação é o único aviso de que alguém abriu uma tela — e é
 * exatamente por isso que ela não pode disparar à toa. Notificar quem está
 * olhando para a janela, ou avisar a própria pessoa sobre a própria tela, é o
 * tipo de ruído que ensina o usuário a desligar notificações.
 */
describe('quando avisar que uma tela abriu', () => {
  const eu = 'ana';
  const outra = 'bruno';

  it('avisa quando outra pessoa transmite e a janela está escondida', () => {
    expect(shouldNotify({ publisherId: outra, selfId: eu, windowFocused: false })).toBe(true);
  });

  it('cala quando a janela está em foco', () => {
    expect(shouldNotify({ publisherId: outra, selfId: eu, windowFocused: true })).toBe(false);
  });

  it('nunca avisa a própria pessoa sobre a própria tela', () => {
    expect(shouldNotify({ publisherId: eu, selfId: eu, windowFocused: false })).toBe(false);
  });

  it('avisa mesmo sem saber quem somos', () => {
    // Acontece na janela entre o gateway conectar e o READY chegar. Silenciar
    // por precaução perderia justamente o aviso que interessa.
    expect(shouldNotify({ publisherId: outra, selfId: undefined, windowFocused: false })).toBe(
      true,
    );
  });
});
