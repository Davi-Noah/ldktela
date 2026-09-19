# ADR-0020 — O transporte é comum: não há adversário de rede

- **Status:** Aceito
- **Data:** 2026-09-14
- **Revisto:** 2026-09-19 (ver nota ao fim)
- **Rebaixa:** [ADR-0013](0013-turn-tls-443-primario.md)

## Contexto

O [ADR-0013](0013-turn-tls-443-primario.md) foi escrito sobre uma inferência, e ela estava
errada: tratava o tráfego de mídia em tempo real como algo que a rede tentaria descartar ou
degradar, e daí tirou TURN/TLS em 443 como caminho de primeira classe, um IP público
dedicado só para ele, e a Fase 3 do `DESTRAVAR.md` como o teste que decidiria se o produto
existe.

Em 2026-09-14 a premissa foi corrigida por quem conhece o ambiente de uso: **a rede
transporta mídia em tempo real normalmente.** Não há um adversário de transporte a
atravessar; o requisito de rede é o de qualquer produto de WebRTC.

## Decisão

A justificativa existencial do ADR-0013 cai. A decisão técnica dele **não** cai inteira — é
rebaixada de requisito de sobrevivência para engenharia normal:

1. **TURN continua necessário**, pelo motivo comum a todo produto de WebRTC: há usuários
   atrás de CGNAT e de rede corporativa que não conectam sem relay. O SRS v1.2 §8 já listava
   isso como risco próprio. Sem TURN, esses usuários específicos não usam o produto — o que é
   grave, mas é "alguns usuários não conectam", não "o produto não tem razão de existir".
2. **A porta 443 e o IP dedicado deixam de ser obrigatórios.** Sem a premissa antiga, TURN/TLS
   em 5349 resolve o caso do CGNAT, e a fatia S2 perde o segundo IP público, o segundo
   certificado e a multiplexação de porta com o Caddy. Fica como preferência: 443 ainda é a
   porta que mais atravessa firewall corporativo, e se for barato de obter, continua sendo a
   melhor escolha.
3. **A Fase 3 deixa de ser o portão do projeto.** O teste com UDP bloqueado passa a ser o que
   sempre deveria ter sido: a verificação de que o caminho relayado funciona, executada junto
   de S2, e não um veredito sobre a existência do produto.
4. **O [ADR-0018](0018-construir-antes-de-medir.md) é quitado em parte.** A dívida que ele
   registrava era "nenhum número medido". A Fase 2 produziu os números (`docs/RESULTS.md`,
   2026-09-14) e eles passam em RNF-02, RNF-03, RNF-04 e RNF-05. Resta um item inconclusivo, os
   congelamentos.

## Consequências

- **O risco número um do projeto muda de lugar.** Não é mais transporte. Passa a ser, nesta
  ordem: (a) os 139 s de congelamento em 26 min que o `RESULTS.md` registra honestamente como
  **inconclusivos**, e que precisam de uma medição limpa com o espectador em outra máquina;
  (b) o áudio, porque o caso de uso central é assistir gameplay e o
  [ADR-0014](0014-audio-por-aplicativo.md) ainda não existe — hoje compartilhar com áudio
  retransmite a voz do Discord de todos de volta para eles.
- **A margem de latência medida não é a margem de produção.** Os 212 ms de p95 foram obtidos com
  as três pontas na mesma máquina, RTT de 2–3 ms. Um usuário real contra uma VM soma o RTT de
  verdade — o próprio SRS v1.2 estimava 35–45 ms entre Nordeste e Sudeste, o que põe o p95 na
  casa dos 250 ms. Continua passando o RNF-02, com folga de ~15% em vez dos 29% medidos. Vale
  saber antes de gastar essa folga em outra coisa.
- S2 fica mais barata e sai da frente: uma VM comum com Caddy, LiveKit e TURN na porta padrão
  resolve.
- Se algum dia aparecer um problema de transporte de fato, este ADR é substituído e o ADR-0013
  volta à força total.

## Alternativas rejeitadas

- **Manter a Fase 3 como portão "por precaução".** Seria travar o projeto num teste caro contra
  uma ameaça que quem conhece o ambiente afirma não existir. Precaução sem hipótese é só atraso.
- **Descartar o ADR-0013 inteiro e tirar o TURN do roadmap.** Jogaria fora a parte da decisão
  que continua certa por outro motivo. CGNAT existe independentemente de qualquer outra coisa.

## Nota de revisão (2026-09-19)

Este ADR foi reescrito antes de o repositório se tornar público. O título e o contexto
descreviam a premissa corrigida em termos do ambiente regional de uso; isso saiu, porque não
é uma decisão de engenharia e não pertence a um registro de arquitetura. A decisão, as
consequências, os números e o raciocínio técnico são os originais. O texto anterior está no
histórico do git.
