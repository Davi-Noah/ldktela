# ADR-0006 — Anexos em tabela própria

- **Status:** Aposentado em 2026-09-12 — o produto não tem anexos
  (ver [ADR-0008](0008-complemento-ao-discord.md))
- **Data:** 2026-08-29 (originado no SRS v1.2 §2.1)

## Contexto

Anexos podiam viver como `messages.attachments JSONB` ou em tabela própria. O pipeline de
espelhamento reescrevia URLs do Discord para o R2 e precisava de coleta de órfãos após
deleções, o que exige consultar anexos por chave de objeto, não por mensagem.

## Decisão original

Anexos em tabela `attachments` própria, com `r2_key` indexável.

## Por que foi aposentado

O pivô de 2026-09-12 remove mensagens, anexos e armazenamento de objetos do produto. Não
há mais o que anexar: o único conteúdo que trafega é o stream de tela, que não é
persistido em lugar nenhum.

Consequências da aposentadoria, já previstas no
[ADR-0016](0016-poda-por-reescrita-de-migrations.md): saem a tabela `attachments`, o
módulo `crates/api/src/storage.rs`, o job de coleta de órfãos e as dependências
`aws-sdk-s3` / `aws-config` / `aws-credential-types`. O Cloudflare R2 permanece em uso
**apenas** como destino do `pg_dump` diário, o que é feito por ferramenta de sistema no
cron e não pelo binário da aplicação.
