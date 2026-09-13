# ADR-0013 — TURN/TLS em 443 é caminho primário, não fallback

- **Status:** Aceito
- **Data:** 2026-09-12

## Contexto

Este é o ADR mais importante do produto, porque descreve a única premissa que, se falsa,
o mata.

O produto existe porque o compartilhamento de tela do Discord não funciona na região
alvo. A causa mais provável não é uma trava dentro do aplicativo Discord: é a rede
descartando ou degradando o tráfego de mídia em tempo real. **Nossa mídia enfrenta
exatamente o mesmo adversário de rede.**

A configuração padrão de WebRTC é a mais frágil possível diante disso: UDP em portas
altas (50000–60000 na config atual do LiveKit), que é a primeira coisa que um filtro
derruba. TURN em 5349 é porta registrada e classificável em uma linha de regra. O SRS v1.2
tratava TURN/TLS como mitigação de CGNAT (§8) — uma preocupação real, mas menor que esta.

## Decisão

**TURN sobre TLS na porta 443 é um caminho de primeira classe, sempre disponível e sempre
exercitado**, não um fallback acionado quando tudo mais falha.

Consequência de infraestrutura: 443 já é do Caddy. O TURN recebe um **IP público
dedicado** na VM, com hostname e certificado próprios, para não disputar a porta.

O critério de aceite é mensurável e obrigatório antes de qualquer outra fatia de mídia:
**uma sessão 1080p estabelece e se sustenta por 20 minutos com todo o UDP de saída
bloqueado no cliente.** Isso é verificável com uma regra de firewall local, não precisa da
região alvo para ser testado.

## Consequências

- O egress **não** dobra. Como o TURN e o SFU rodam na mesma VM, o repasse do cliente
  relayado para o SFU acontece localmente; o que sai da VM é o mesmo de antes. O custo real
  é CPU e memória de relay, que precisam entrar no orçamento da A1.Flex.
- O caminho relayado tem latência maior que o direto. O RNF de glass-to-glass precisa ser
  medido **nesse** caminho, não no melhor caso, porque na região alvo ele é o caso comum.
- Exige um IP público secundário na VM e configuração de TLS separada da do Caddy.
- Portas UDP diretas continuam habilitadas: onde a rede permite, elas são melhores. A
  decisão é sobre garantir o caminho pior, não sobre abandonar o melhor.
- Nada aqui é técnica de evasão: TURN/TLS em 443 é o mecanismo padrão que todo produto de
  videoconferência usa para atravessar CGNAT e firewall corporativo. O que muda é a
  prioridade que damos a ele.

## Alternativas rejeitadas

- **UDP com fallback para TCP em 7881.** 7881 é tão classificável quanto 5349, e um
  fallback que só é exercitado em produção é um fallback que não funciona.
- **Serviço de TURN de terceiros.** Coloca um terceiro no caminho da mídia, tira o
  controle do egress e adiciona uma dependência externa ao componente mais crítico.
- **Tratar isso como fatia tardia de "endurecimento".** Se falhar, o produto inteiro não
  tem razão de existir. Vai para o começo do roadmap, antes de qualquer investimento em UI.
