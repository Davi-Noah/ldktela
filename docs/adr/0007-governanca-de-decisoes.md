# ADR-0007 — Decisões registradas como ADR numerado

- **Status:** Aceito
- **Data:** 2026-09-12

## Contexto

O desenvolvimento acontece majoritariamente em harness de IA, em sessões independentes
que não compartilham contexto. Até aqui as decisões viviam em dois lugares: um bloco
"ADR resumido" dentro do SRS (§2.1, seis entradas) e um log corrido em `docs/DECISIONS.md`
(mais de cem entradas por estágio).

Os dois funcionaram, mas o bloco no SRS ficou congelado — nenhuma decisão nova entrou
nele desde a redação original — e o log corrido mistura granularidades: "a ponte é
permanente" e "`argon2` fixado em 0.5" moram na mesma lista, com o mesmo peso visual.
O resultado prático é que decisões estruturais ficam difíceis de encontrar exatamente
quando alguém está prestes a contrariá-las.

## Decisão

Toda decisão que restrinja trabalho futuro é registrada como um ADR numerado em
`docs/adr/NNNN-titulo.md`, **antes de a tarefa que a originou ser dada como pronta**.

`docs/DECISIONS.md` continua existindo, com escopo estreitado: notas de implementação que
explicam por que uma linha específica é como é. O critério de separação está no
`docs/adr/README.md`.

## Consequências

- `docs/adr/README.md` passa a ser o índice único de decisões estruturais, com status
  explícito por entrada.
- Um ADR revertido nunca é apagado: vira `Substituído por ADR-XXXX`. O histórico de
  decisões erradas é parte do valor do registro.
- Custo assumido: escrever o ADR é trabalho a mais em cada tarefa, e ADRs desatualizados
  são piores que ausentes. O status explícito é o que mantém isso administrável.
- `CLAUDE.md` §12 (definição de pronto) passa a exigir o ADR como item de aceite.

## Alternativas rejeitadas

- **Manter tudo em `docs/DECISIONS.md`.** Já se provou insuficiente: cem entradas de
  granularidade misturada não são consultáveis no momento da decisão.
- **Manter as decisões no SRS.** O SRS descreve o sistema como ele deve ser; um ADR
  descreve por que ele não é de outro jeito. Misturar os dois faz o SRS crescer sem
  parar e perde a data de cada escolha.
