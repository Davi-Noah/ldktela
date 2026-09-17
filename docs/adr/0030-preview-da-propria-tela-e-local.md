# ADR-0030 — O preview da própria tela é local, nunca pelo SFU

- **Status:** Aceito
- **Data:** 2026-09-16

## Contexto

Até aqui, quem compartilhava não via nada do que estava enviando. O
[ADR-0027](0027-publicador-e-um-segundo-participante.md) faz o publicador entrar na sala
como um segundo participante (`~pub`), e por isso a nossa própria tela **chega** à conexão
de espectador como se fosse de outra pessoa. O `MediaSession` descarta essa publicação de
propósito, com um `return` explícito.

Duas pressões se somaram:

1. O modo de falha número um de compartilhamento de tela é mostrar a janela errada, e o
   número dois é o outro lado receber quadro preto ou congelado. Sem imagem, o aplicativo
   tentava responder isso com números — o aviso de "capturada mas não codificada" no painel
   do publicador existe só porque não havia figura.
2. O usuário do produto é usuário do Discord, onde a própria tela aparece como mais um
   ladrilho da grade. Sem ela, começar a transmitir não muda nada na tela: o aplicativo
   continua mostrando a lista de participantes.

Existia um caminho tentador e errado: apagar aquele `return` e assinar o próprio track.

## Decisão

**O preview da própria tela vem do core Rust, dos quadros que a captura já tem em mãos, e
nunca de uma assinatura do próprio track no SFU.**

O `return` em `MediaSession.adoptPublication` que ignora a identidade `~pub` própria é
carga estrutural. Não remova.

A derivação acontece em `capture.rs`, depois de o quadro ser entregue ao encoder: um
ramo opcional subamostra o BGRA, uma thread separada codifica em JPEG e emite
`share://preview` para o WebView, que troca o `src` de um `<img>` criado
imperativamente. O ramo descarta quadro quando a codificação fica para trás, e **para na
origem** quando o usuário desliga o preview — não é `display:none`.

**Resolução e relógio acompanham o contexto**, os dois: 480 px a 3 fps enquanto o preview
é um ladrilho da grade, 1280 px a 12 fps quando ele está em foco.

> **Corrigido em 2026-09-17.** A primeira versão variava só o relógio e fixava 480 px nos
> dois casos. Em foco o preview ocupa a janela inteira, então 480 px viravam um aumento de
> quase três vezes, e o resultado era borrado o bastante para o dono do projeto concluir
> que **a transmissão** estava com qualidade "comicamente baixa" — e abrir uma investigação
> de rede por causa disso. O erro não foi o número: foi variar uma dimensão do problema
> (fps) e esquecer a outra (pixels), quando as duas mudam pelo mesmo motivo.

## Consequências

**Fica fácil:** mostrar a própria tela sem nenhum custo de rede; enxergar a fonte errada
em menos de um segundo; dar ao seletor as mesmas miniaturas pela mesma função de
subamostragem e codificação.

**Fica caro:** o preview passa a ser mais uma coisa que pode quebrar no core, e o teste
dela é manual, porque depende de uma tela real.

**Fica proibido:** assinar a própria publicação. O custo não é só egress mais ingress de
pixels que já estão na máquina — com um inscritor permanente, o `dynacast` deixa de pausar
o encoder quando ninguém assiste, e passa-se a pagar cerca de um núcleo de codificação
(número do próprio `RESULTS.md`) para transmitir para si mesmo. `syncViewers` também
precisaria de mais uma exceção para não contar você como seu próprio espectador.

**Fica proibido, também:** fazer o preview atravessar o React quadro a quadro. Vale aqui a
mesma regra do `<video>` (CLAUDE.md §7): o elemento é criado uma vez e escrito de fora.

## Alternativas rejeitadas

**Assinar o próprio track no SFU.** Uma linha a menos de código e três custos a mais:
egress, ingress e um encoder que nunca pausa. Detalhado acima.

**Um segundo `NativeVideoSource` local, sem sala.** Não existe consumidor: o WebView não
fala com o SDK Rust, e o único transporte de vídeo entre os dois é o próprio SFU.

**Enviar o quadro cru (BGRA/RGBA) pelo IPC.** 480×270 em RGBA são 518 KB por quadro,
contra cerca de 20 KB em JPEG q70. A 12 fps, é a diferença entre 6 MB/s e 240 KB/s de
tráfego de IPC para uma imagem que vai ser desenhada e descartada.

**Só uma foto no início, sem preview contínuo.** Foi considerado como primeira fase e
cobriria a pergunta "escolhi a janela certa?". Não cobre a segunda, que é "continua indo?"
— e é essa que aparece depois de vinte minutos, quando a janela foi fechada, o jogo entrou
em tela cheia exclusiva ou o capturador desistiu em silêncio.
