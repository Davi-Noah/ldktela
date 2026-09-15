import { describe, expect, it } from 'vitest';
import pkg from '../../package.json';

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
