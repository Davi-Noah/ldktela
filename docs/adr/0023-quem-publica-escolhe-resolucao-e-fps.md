# ADR-0023 — Quem publica escolhe resolução e fps; quem assiste escolhe a camada

- **Status:** Aceito
- **Data:** 2026-09-14

## Contexto

O S7 pede, no espectador, um seletor de resolução (720p/1080p) e outro de taxa de
quadros (30/60). Isso não é implementável como escrito, e implementar assim mesmo
seria pior do que não ter.

Simulcast é decidido por **quem publica**. O espectador só pode pedir entre as
camadas que já estão sendo enviadas. Hoje o publicador manda duas — 1080p60 e
720p30 — então a combinação "1080p a 30 fps" simplesmente não existe para
escolher. Um seletor com quatro opções teria duas que não fazem nada.

Para o espectador escolher de verdade entre 2 resoluções × 2 taxas, o publicador
teria de codificar **quatro** camadas. O encode já custa ~1 núcleo em 1080p60
(`RESULTS.md`, RNF-04); quatro camadas no mínimo dobram isso, e o custo cai sobre
uma pessoa para dar opção às outras.

## Decisão

Os dois controles existem, em lados diferentes, e a interface diz de quem é cada um.

**No publicador** — resolução e taxa de quadros, porque é ele quem paga o encode:
1080p60, 1080p30, 720p60, 720p30. A escolha define a camada alta; a baixa é
derivada automaticamente (metade da resolução, 30 fps).

**No espectador** — automático, alta ou baixa, entre o que estiver chegando.
`adaptiveStream` continua podendo descer abaixo de uma escolha fixa quando a
janela é pequena ou está oculta: economizar banda de quem não está olhando vence
a preferência declarada.

## Consequências

- O seletor do espectador nunca mente. Ele lista o que existe.
- **A grade assina a camada baixa; só o foco pede a alta.** Isto não é
  refinamento, é o que separa 190 h de 380 h no orçamento de egress: com N
  telas visíveis ao mesmo tempo, o custo multiplica por N, e `adaptiveStream` só
  ajuda se o layout de fato pedir tamanho pequeno para o que está pequeno.
- Trocar a resolução ou o fps enquanto se compartilha exige republicar a track.
  A interface precisa dizer isso, ou o usuário acha que travou.
- O teto de publicadores por sala (`ROOM_MAX_PUBLISHERS`, P-01) passa a ter
  consequência direta de custo, não só de CPU do SFU. Sobe de 2 só com o número
  de egress na mão.

## Alternativas rejeitadas

- **Quatro camadas no publicador.** Dá ao espectador a escolha literal que foi
  pedida, ao preço de dobrar o custo de quem compartilha. Trocar CPU de uma
  pessoa por conveniência das outras é a troca errada quando quem compartilha já
  está rodando um jogo.
- **Seletor de fps no espectador que não faz nada.** Seria descobrir o problema
  pelo suporte, meses depois, em vez de na especificação.
