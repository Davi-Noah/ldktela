# ADR-0004 — IDs UUIDv7 gerados na aplicação

- **Status:** Aceito
- **Data:** 2026-08-29 (originado no SRS v1.2 §2.1, C-08)

## Contexto

UUIDv4 é aleatório: fragmenta o B-tree a cada inserção, infla índices e não oferece
ordenação temporal, o que obriga a uma coluna auxiliar para paginar de forma estável.

## Decisão

Todas as chaves primárias são UUIDv7 geradas na aplicação com `Uuid::now_v7()`. O schema
**não** define `DEFAULT` para chave primária.

## Consequências

- Ordenação temporal natural: a própria PK ordena por tempo de criação.
- Localidade de índice na inserção, sem fragmentação.
- Paginação por keyset sem coluna auxiliar — regra que continua valendo mesmo com o
  schema reduzido do pivô.
- O ID é conhecido pela aplicação **antes** do `INSERT`, o que simplifica gravar relações
  numa transação só.

## Alternativas rejeitadas

- **UUIDv4 com `DEFAULT gen_random_uuid()` no banco.** Fragmentação e perda de ordenação.
- **`BIGSERIAL`.** Expõe volume e ordem de criação a quem lê a API, e complica geração
  distribuída se o processo deixar de ser único.
