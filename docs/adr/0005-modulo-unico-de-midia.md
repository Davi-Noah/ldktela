# ADR-0005 — Aquisição de mídia num único módulo

- **Status:** Aceito
- **Data:** 2026-08-29 (originado no SRS v1.2 §2.1)

## Contexto

Chamadas a `getDisplayMedia` espalhadas pelos componentes tornariam qualquer mudança de
estratégia de captura uma refatoração transversal — e a estratégia de captura é
justamente a parte do produto com mais probabilidade de mudar.

## Decisão

Toda aquisição de mídia é encapsulada em um único módulo do frontend
(`desktop/src/media/tracks.ts`). Nenhum componente chama a API de captura diretamente.

## Consequências

- Trocar `getDisplayMedia` por um caminho nativo (sidecar, ou o IPC de áudio do
  [ADR-0014](0014-audio-por-aplicativo.md)) é uma mudança local.
- Mantém aberta, a custo zero hoje, a porta para macOS/Linux, onde o WebView do sistema
  não entrega captura com áudio.
- Com o pivô, este módulo deixa de ser um detalhe e passa a ser **o núcleo do produto**:
  é onde vivem `contentHint`, `degradationPreference`, camadas de simulcast e escolha de
  codec.

## Alternativas rejeitadas

- **`getDisplayMedia` direto nos componentes.** Barato hoje, caro exatamente no momento em
  que a estratégia de captura precisar mudar.
