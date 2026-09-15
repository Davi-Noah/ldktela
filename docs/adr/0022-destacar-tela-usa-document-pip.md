# ADR-0022 — Destacar uma tela usa Document Picture-in-Picture

- **Status:** Aceito
- **Data:** 2026-09-14

## Contexto

O S7 pede que as telas compartilhadas possam ser separadas em monitores
diferentes. A leitura óbvia é "abrir N janelas Tauri", e ela é cara de um jeito
que não aparece à primeira vista.

Cada janela Tauri é um **WebView próprio**, com contexto JavaScript próprio. Uma
`MediaStreamTrack` não atravessa essa fronteira. Então cada janela destacada
precisaria da própria conexão com o LiveKit, e isso arrasta:

- identidade distinta por janela, porque o LiveKit derruba a conexão anterior
  quando a mesma identidade entra duas vezes;
- o servidor aceitando várias conexões por usuário, e sabendo desfazer o sufixo
  para voltar ao usuário real;
- `room_presence`, hoje com chave primária em `user_id`, mudando para chave por
  identidade — senão a segunda janela sobrescreve a presença da primeira, e o
  `participant_left` de uma apaga a presença enquanto a outra continua aberta;
- a contagem de espectadores e a lista de participantes deduplicando por usuário.

Uma mudança de schema e de modelo de identidade para uma funcionalidade de
janela.

## Decisão

Destacar usa **Document Picture-in-Picture**
(`window.documentPictureInPicture.requestWindow()`), disponível no Chromium 116+;
o WebView2 desta máquina é 152.

É uma janela do sistema de verdade — arrastável para outro monitor, redimensionável,
sempre visível — que **compartilha o contexto JavaScript da janela principal**. O
elemento `<video>` é movido para dentro dela e devolvido ao fechar.

## Consequências

- Zero conexão extra, zero egress extra, zero mudança de schema, zero mexer em
  identidade. A track já está assinada; só o nó do DOM muda de lugar.
- Respeita a regra do `CLAUDE.md` §7 de nunca remontar o elemento de vídeo:
  mover um nó entre documentos com `adoptNode`/`append` preserva o elemento e,
  com ele, o decodificador. Remontar custaria segundos de tela preta.
- **Limite aceito: uma janela destacada por vez**, além da principal. A
  especificação do Document PiP permite uma por documento. Para duas telas em
  dois monitores além do principal, seria preciso voltar às janelas Tauri.
- O CSS não é herdado pela janela de PiP: os estilos precisam ser copiados ou
  reinjetados. É trabalho conhecido, não risco.

## Alternativas rejeitadas

- **N janelas Tauri.** Mais poderoso e bem mais caro, pelos motivos do contexto.
  Fica registrado como o caminho a seguir **se** o uso real mostrar que uma
  janela destacada não basta — e aí com ADR novo, porque a mudança de
  `room_presence` é estrutural.
- **Tela cheia no monitor secundário.** Não resolve: tela cheia ocupa um monitor
  inteiro com uma tela só, e o pedido é ver várias ao mesmo tempo em lugares
  diferentes.
