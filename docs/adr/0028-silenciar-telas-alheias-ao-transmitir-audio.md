# ADR-0028 — Quem transmite áudio não ouve o áudio das outras telas

- **Status:** Aceito
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
