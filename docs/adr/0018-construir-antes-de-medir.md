# ADR-0018 — Construir S1 e S3–S6 antes de medir S0

- **Status:** Aceito, com dívida explícita
- **Data:** 2026-09-13

## Contexto

O `docs/ROADMAP.md` põe **S0 — spike de viabilidade** antes de tudo, e o
justifica com a história do projeto: o SRS da v1 mandava fazer o spike de screen
share primeiro, isso foi ignorado, e onze estágios de plano de controle foram
construídos sem nunca verificar se o produto era viável.

Em 2026-09-13 o dono do projeto autorizou executar o pivô e pediu explicitamente
para "tornar esse produto funcional", com "total liberdade de implementação".

O conflito é real. S0 exige duas máquinas Windows físicas, uma rede onde bloquear
UDP de saída, e a leitura de latência glass-to-glass e CPU sob carga — trabalho
que um agente não executa. Esperar por ele significaria não entregar nada.

## Decisão

A fatia S1 (poda) e as metades de backend de S3, S4 e S5, mais o cliente desktop
de S6, foram construídas **antes** de S0.

A obrigação de medir **não** foi cancelada, e o alvo dela mudou: em vez de código
descartável em `spike/`, a medição passa a ser feita contra o cliente real, que
agora existe. O `spike/` deixa de ser necessário.

## Consequências

- O produto existe de ponta a ponta e pode ser medido de verdade, o que é melhor
  que medir uma POC que não é o produto.
- **A dívida permanece e é a mesma:** continua sem existir um único número medido
  de bitrate, egress, latência glass-to-glass ou CPU neste repositório, e a
  premissa do [ADR-0013](0013-turn-tls-443-primario.md) — a de que nossa mídia
  atravessa uma rede que bloqueia a do Discord — **continua não verificada**.
- O critério de aceite de S0 é transferido para S6 sem abrandamento: sessão
  1080p60 estável por 30 min entre duas máquinas, **e** a mesma sessão sustentada
  por 20 min com todo o UDP de saída bloqueado no cliente, com os números em
  `docs/RESULTS.md`.
- Enquanto isso não for feito, nenhuma parte do produto pode ser declarada
  pronta. `docs/ROADMAP.md` registra isso no topo.
- O risco aceito é concreto: se a medição reprovar, parte do cliente muda de
  forma. O que a torna aceitável é que a arquitetura de mídia está isolada em
  `desktop/src/media/tracks.ts` ([ADR-0005](0005-modulo-unico-de-midia.md)), que
  é exatamente o lugar que uma reprovação obrigaria a reescrever.

## Alternativas rejeitadas

- **Fazer S0 primeiro, e parar.** Entregaria zero código executável e deixaria a
  medição igualmente por fazer, porque quem a executa é uma pessoa com duas
  máquinas, não o agente.
- **Escrever o spike descartável mesmo assim.** Seria um segundo cliente de
  mídia para manter até S6, medindo um pipeline que não é o que vai para
  produção. O `spike/` fazia sentido para de-riscar antes de investir; depois do
  investimento, ele é só duplicação.
- **Declarar o produto pronto.** Seria repetir o erro da v1 com outro nome.
