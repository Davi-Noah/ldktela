# ADR-0028 — Quem transmite áudio não ouve o áudio das outras telas

- **Status:** Aceito, restringido em 2026-09-20 e ampliado em 2026-09-23 (ver emendas ao fim)
- **Data:** 2026-09-14
- **Decorre de:** [ADR-0025](0025-audio-exclui-o-discord.md)

## Contexto

`AUDIOCLIENT_ACTIVATION_PARAMS` aceita **um** identificador de processo. Uma árvore
excluída, não uma lista. O [ADR-0025](0025-audio-exclui-o-discord.md) gasta essa única
exclusão no Discord, que é o requisito do S9.

Sobra um caso. O nosso próprio aplicativo também toca som: o WebView2 reproduz o áudio das
telas dos outros, e o processo do WebView2 é filho do nosso. Como o teto de publicadores é
2 ([ADR-0012](0012-midia-unidirecional.md)), duas pessoas podem compartilhar com áudio ao
mesmo tempo — e aí a captura de A pega o áudio de B e o devolve para a sala. B se ouve com
atraso, e todo mundo recebe B duas vezes.

É o mesmo defeito que o [ADR-0014](0014-audio-por-aplicativo.md) foi escrito para matar,
numa esquina mais estreita.

## Decisão

Enquanto o usuário está transmitindo **com áudio**, o cliente silencia localmente o áudio
das telas alheias, e diz isso na interface.

A condição é exatamente essa: transmitir áudio. Assistir sem compartilhar não silencia
nada, e compartilhar sem áudio também não.

## Consequências

- Some o eco, e some a duplicação para quem assiste.
- **Em troca, quem compartilha áudio não ouve o áudio da outra tela.** É perda real, e
  restrita ao caso de dois publicadores com áudio simultâneos. Volta no instante em que a
  transmissão para.
- A voz do Discord não é afetada: o Discord está fora da nossa captura por construção
  ([ADR-0025](0025-audio-exclui-o-discord.md)) e fora do nosso silenciamento por não ser
  nosso.

## Alternativas rejeitadas

- **Excluir a nossa árvore em vez da do Discord.** Traz de volta a voz de todo mundo na
  transmissão, que é o problema que o S9 existe para resolver.
- **Duas capturas e subtrair uma da outra** — uma excluindo o Discord, outra incluindo só o
  nosso processo. São fluxos WASAPI independentes, sem alinhamento de amostra nem de ganho
  garantidos; seria cancelamento de eco caseiro, frágil, num caminho onde falhar é audível.
- **Deixar ecoar e avisar.** O [ADR-0014](0014-audio-por-aplicativo.md) já registra que
  aviso não conserta eco, só transfere a culpa.

## Emenda de 2026-09-20 — compartilhar uma janela não silencia ninguém (issue #11)

A decisão acima vale para o caso em que ela foi escrita: **a tela inteira**. Aí a captura é a
máquina toda menos o Discord, o áudio das telas alheias sai pelo mesmo alto-falante que ela grava,
e silenciar é o que impede a volta.

Compartilhar uma **janela** não é esse caso. `AUDIOCLIENT_ACTIVATION_PARAMS` aceita também
`INCLUDE_TARGET_PROCESS_TREE`, e a janela compartilhada tem dono: o `HWND` que a identifica é o
mesmo que dá o processo. Incluindo só essa árvore, o que sai é o som daquela janela e nada mais —
nem o Discord, nem o navegador ao lado, nem as telas dos outros. Não há o que silenciar, e
silenciar mesmo assim tira da pessoa um áudio sem nenhuma razão técnica.

Então: **o silenciamento passa a depender do modo de captura que o core conseguiu**, e não do fato
de transmitir áudio. `only_window` não silencia; `excluding_discord` e `whole_system` continuam
silenciando, pelo motivo original.

A degradação importa e é deliberada: se o modo por janela falhar, o core cai para
`excluding_discord` — e não para o sistema inteiro — e relata isso, de modo que a interface volta a
silenciar sozinha. O inverso, cair calado para uma captura que pega tudo sem silenciar nada, é
exatamente o eco que este ADR existe para impedir.

Uma consequência fica de pé: a janela compartilhada continua sendo a única fonte de som. Quem
espera mandar o jogo numa janela e a música de outro programa junto não consegue — é o preço de
mandar só o que foi escolhido, e o seletor diz isso antes de começar.

## Emenda de 2026-09-23 — o nosso aviso sonoro também se cala

O contexto acima já dizia a frase inteira: *o nosso próprio aplicativo também toca som*. A decisão
silenciou a parte desse som que vinha dos outros — as telas alheias — e deixou de fora a parte que
é nossa: o sino de uma tela que entra ou sai. Ele é sintetizado no mesmo WebView2, sai pelo mesmo
alto-falante, e a captura da tela inteira o grava do mesmo jeito.

O defeito relatado foi exatamente esse: alguém começa a transmitir a tela com áudio enquanto ouve
outra tela com áudio, e o sino de "tela começou" entra num laço cada vez mais alto e distorcido,
que só termina quando alguém silencia o aplicativo por um instante. O sino entrava na transmissão,
voltava pelo áudio de quem assiste e realimentava o laço.

Então: **enquanto o silenciamento desta decisão estiver valendo, o sino também não toca.** A
condição é a mesma, avaliada no mesmo lugar (`shouldSilenceOtherScreens`), porque a razão é a mesma:
nada que este processo toque pode sair pelo alto-falante enquanto a nossa captura grava este
processo. Compartilhar uma janela continua não silenciando nada, e continua com o sino.

**Em troca, quem transmite a tela inteira com áudio não ouve o sino.** A notificação do sistema
continua valendo quando a janela não está em foco, e o ladrilho novo aparece na grade — o sino era
a forma de saber sem olhar, e é ela que se perde durante a transmissão.

Qualquer som novo que o aplicativo venha a tocar entra na mesma regra.
