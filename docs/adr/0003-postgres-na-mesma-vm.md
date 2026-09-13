# ADR-0003 — PostgreSQL auto-hospedado na mesma VM

- **Status:** Aceito — ver [ADR-0015](0015-postgres-com-schema-reduzido.md) para o
  redimensionamento pós-pivô
- **Data:** 2026-08-29 (originado no SRS v1.2 §2.1, C-02)

## Contexto

A alternativa gerenciada em camada gratuita (Supabase, Neon) oferece ~500 MB, que na v1
não comportaria histórico de mensagens nem índice de busca full-text. A VM traz um volume
de bloco de 200 GB no mesmo plano gratuito.

## Decisão

PostgreSQL 16 roda auto-hospedado, na mesma VM do backend e do SFU.

## Consequências

- Sem teto artificial de armazenamento e sem restrição a índices caros.
- Backup e disponibilidade passam a ser responsabilidade própria: `pg_dump` diário para
  fora do provedor, com teste de restauração mensal registrado (RNF-08). Backup não
  testado não é backup.
- A VM vira ponto único de falha para banco, API e SFU ao mesmo tempo.

## Alternativas rejeitadas

- **Postgres gerenciado em free tier.** 500 MB, e a saída dessa cota é uma fatura, não um
  aviso.
