import { DisconnectReason } from 'livekit-client';
import { describe, expect, it } from 'vitest';
import { shouldRejoin } from './tracks';

/**
 * Guarda a única desconexão que não se deve tentar de novo.
 *
 * Existe por causa de uma falha real, e o log do SFU a descreve melhor do que
 * qualquer explicação: cinco entradas e quatro remoções por `DUPLICATE_IDENTITY`
 * em quatro segundos, na mesma identidade, com `peak_viewers` parado em zero. Os
 * dois clientes estavam pareados na mesma conta, e cada reconexão expulsava o
 * outro — que reconectava e expulsava de volta.
 *
 * O sintoma para quem usa é o pior possível: o usuário pisca na lista entrando e
 * saindo, a tela compartilhada nunca aparece, e o painel mostra 0 kb/s e 0 fps,
 * porque o dynacast pausa o encoder enquanto não existe assinante estável. Nada
 * disso parece "a mesma conta está aberta em dois lugares".
 */
describe('quando vale reconectar à sala de mídia', () => {
  it('não reconecta quando a mesma conta entrou de outro lugar', () => {
    expect(shouldRejoin(DisconnectReason.DUPLICATE_IDENTITY)).toBe(false);
  });

  it('reconecta em tudo o mais, inclusive sem motivo declarado', () => {
    // Queda de rede, servidor reiniciado, token expirado: todos se resolvem
    // sozinhos tentando de novo, e desistir deixaria a sala vazia à toa.
    for (const reason of [
      undefined,
      DisconnectReason.UNKNOWN_REASON,
      DisconnectReason.SERVER_SHUTDOWN,
      DisconnectReason.STATE_MISMATCH,
      DisconnectReason.JOIN_FAILURE,
      DisconnectReason.ROOM_DELETED,
    ]) {
      expect(shouldRejoin(reason), `motivo ${String(reason)}`).toBe(true);
    }
  });
});
