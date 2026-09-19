# ADR-0017 — O produto não é uma Discord Activity

- **Status:** Aceito
- **Data:** 2026-09-12

## Contexto

Depois do reposicionamento do [ADR-0008](0008-complemento-ao-discord.md), a pergunta
aparece sozinha e é boa: se o produto é um complemento ao Discord, por que não vive
*dentro* do Discord, como uma Activity na aba de atividades, em vez de exigir instalação
de um aplicativo desktop?

O apelo é real: atrito de instalação zero, descoberta dentro do cliente que o usuário já
tem aberto, e nenhuma segunda janela para gerenciar. Se funcionasse, seria melhor que o
que está no roadmap.

## Decisão

O produto **não** é implementado como Discord Activity. Continua sendo um aplicativo
desktop Tauri, conforme o SRS v2.0 §1.4.

Há dois motivos independentes, e cada um sozinho basta.

### 1. Bloqueio técnico: Activities não suportam WebRTC

A documentação de rede de Activities do Discord é explícita: *"WebRTC is not supported."*
O único transporte disponível é WebSocket; WebTransport está listado como em
desenvolvimento junto aos provedores upstream, sem prazo. Todo o tráfego de uma Activity
ainda passa por um proxy de sandbox do Discord, que existe para esconder o IP do usuário
e bloquear endpoints, e que exige mapeamento de URL declarado no portal.

Nosso produto **é** WebRTC: LiveKit SFU, SRTP, simulcast, `adaptiveStream`, `dynacast`,
TURN. Nada disso existe sobre WebSocket. Implementar vídeo 1080p60 sobre WebSocket
significaria reescrever, pior, controle de congestionamento, jitter buffer, adaptação de
bitrate e caminho de decodificação por hardware — sobre um transporte que não foi feito
para isso, atrás de um proxy que não foi feito para mídia. Não é uma versão mais simples
do produto; é um produto inviável.

O `getDisplayMedia` dentro do iframe é discussão que nem chega a acontecer: sem transporte
de mídia, não importa se a captura é possível.

### 2. Dependência estratégica: o lugar errado para estar

Este motivo é independente do primeiro e sobrevive a ele. Se o Discord habilitar WebRTC em
Activities amanhã, a resposta continua não.

Uma Activity roda dentro do cliente do Discord, é distribuída pelo Discord, passa por
revisão do Discord e pode ser removida pelo Discord. Seria construir o produto inteiro
sobre um recurso que a plataforma controla e que ela pode recusar ou retirar a qualquer
momento: reprovação na revisão no melhor caso, e um único ponto de remoção no pior.

O [ADR-0008](0008-complemento-ao-discord.md) escolheu deliberadamente o oposto: cada
comunidade hospeda a própria instância, sem ponto central de controle. Virar Activity
trocaria essa propriedade — que é boa parte da razão de o produto ser viável — por
conveniência de instalação.

### 3. Custo colateral

Como Activity, perdemos ainda: a captura de áudio por aplicativo via WASAPI
([ADR-0014](0014-audio-por-aplicativo.md)), que é o maior diferencial do produto e é
impossível num iframe; o cofre do sistema operacional para o refresh token; a bandeja e a
operação em segundo plano; e o controle sobre o caminho de transporte, que é justamente
o requisito existencial do [ADR-0013](0013-turn-tls-443-primario.md).

## Consequências

- O atrito de instalação continua existindo e é aceito como custo do produto. O
  [ADR-0011](0011-sala-e-o-canal-de-voz.md) e a fatia S8 reduzem esse atrito por outro
  caminho: o anúncio no Discord com link profundo leva o usuário direto à sala, e quem não
  tem o aplicativo cai na página de download.
- **A parte certa da ideia já está no roadmap.** O valor real de estar dentro do Discord é
  a descoberta — "fulano está compartilhando, clique para ver" aparecendo onde as pessoas
  já estão. Isso é a fatia S8, com uma mensagem de bot editada em vigor, e custa uma
  fração de uma Activity sem depender de aprovação de ninguém.
- Se o Discord habilitar WebRTC ou WebTransport em Activities, isto muda de "impossível"
  para "possível e ainda assim indesejável", e a reavaliação exige um ADR novo que
  substitua este — não uma issue.

## Alternativas rejeitadas

- **Activity para tudo.** Bloqueada pela ausência de WebRTC.
- **Híbrido: aplicativo nativo publica, Activity assiste.** Resolveria o atrito para a
  maioria — são muitos espectadores para um publicador. Continua bloqueado pelo mesmo
  motivo: assistir também é WebRTC. E manteria o bloqueio estratégico inteiro.
- **Vídeo sobre WebSocket, ou HLS/LL-HLS dentro da Activity.** Tecnicamente contornável,
  mas HLS de baixa latência vive na casa dos segundos, contra a meta de 300 ms de
  glass-to-glass do RNF-02. Assistir gameplay com segundos de atraso é outro produto.
- **Esperar o WebTransport.** Sem prazo público, e o motivo estratégico permaneceria.

## Fonte

Documentação de rede de Activities do Discord, consultada em 2026-09-12:
<https://docs.discord.com/developers/activities/development-guides/networking>
