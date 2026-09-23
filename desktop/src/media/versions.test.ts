import { describe, expect, it } from 'vitest';
import pkg from '../../package.json';
import { CLIENT_INFO } from '../config';

/**
 * Guarda metade do par de versões do LiveKit (ADR-0019); a outra metade, a
 * imagem do servidor, é verificada em `crates/api/src/livekit.rs`.
 *
 * Existe por causa de uma falha real: `livekit-client` estava declarado como
 * `^2.7.0` e derivou sozinho até 2.22.1, enquanto o servidor seguia fixado em
 * 1.8. O cliente passou a falar um protocolo que o servidor não entende, a
 * negociação de publicação expirava em 15 s e só o compartilhamento quebrava —
 * parear, entrar na sala e assinar continuavam funcionando, e o servidor
 * respondia 200 em tudo. Nada no `just check` tinha como acusar.
 *
 * Este teste não prova compatibilidade: isso exige um SFU de verdade e uma
 * publicação de verdade. Ele prova que o cliente não pode voltar a derivar sem
 * alguém decidir.
 */
describe('o par de versões do LiveKit', () => {
  it('fixa livekit-client numa versão exata, sem faixa', () => {
    const declared: string | undefined = pkg.dependencies['livekit-client'];

    expect(declared, 'livekit-client ausente de package.json').toBeDefined();
    expect(
      declared,
      `livekit-client está como "${declared}". Uma faixa deixa o cliente derivar ` +
        'para longe do servidor e quebra só a publicação. Ver ADR-0019.',
    ).toMatch(/^\d+\.\d+\.\d+$/);
  });
});

/**
 * O gateway recusa cliente abaixo de `MIN_CLIENT_VERSION` fechando com 4010, e
 * é esse fechamento que dispara a atualização automática. A versão que o cliente
 * declara precisa, então, ser a de verdade.
 *
 * Existe por causa de uma falha real: a constante ficou em `0.1.0` por duas
 * versões enquanto o aplicativo já era 1.1.0. Ninguém percebeu porque o mínimo
 * do servidor também era 0.1.0 — a mentira só apareceria na primeira vez que o
 * mínimo subisse, trancando todo mundo para fora de uma vez.
 */
describe('a versão que o cliente declara ao gateway', () => {
  it('é a do package.json, e não uma cópia escrita à mão', () => {
    expect(CLIENT_INFO.version).toBe(pkg.version);
    expect(CLIENT_INFO.version).toMatch(/^\d+\.\d+\.\d+$/);
  });
});
