# ADR-0015 — Postgres mantido apesar do schema reduzido

- **Status:** Aceito
- **Data:** 2026-09-12

## Contexto

O schema da v1 tem 20 tabelas e foi dimensionado para histórico de mensagens e índice de
busca full-text: alvo de 40 GB em 24 meses, num volume de 200 GB
([ADR-0003](0003-postgres-na-mesma-vm.md)).

Depois do pivô sobram cerca de cinco tabelas — usuários (cache do perfil do Discord),
refresh tokens, códigos de pareamento, histórico de sessões de compartilhamento e
contabilidade de egress — e o volume total previsto fica abaixo de 100 MB. Nesse tamanho,
SQLite resolveria, e a pergunta "por que ainda temos um servidor de banco?" é legítima.

## Decisão

Postgres continua. O alvo do RNF-08 cai de 40 GB para menos de 1 GB, e o volume de 200 GB
deixa de ser justificativa para qualquer coisa.

## Consequências

- Nada precisa ser reescrito: o crate `db`, o cache offline do SQLx, o harness de
  `testcontainers` e a suíte de testes com banco real continuam funcionando como estão.
- O `pg_dump` diário fica trivial no novo tamanho, e o teste de restauração mensal
  (RNF-08) passa a ser rápido o bastante para não ser pulado.
- Custo assumido: um processo de banco na VM para poucos megabytes de dados. Em uma VM que
  já roda SFU, TURN e proxy reverso, é ruído.
- O `UNLOGGED` de `voice_states` continua correto e fica ainda mais adequado: estado de
  sala é efêmero por natureza.

## Alternativas rejeitadas

- **Migrar para SQLite.** Reescreveria o crate `db` inteiro, o cache do SQLx e todo o
  harness de teste de integração, para economizar um processo numa VM que sobra recurso.
  Custo alto, benefício nenhum para o usuário.
- **Dispensar o banco e manter tudo em memória.** Refresh tokens e contabilidade de egress
  precisam sobreviver a reinício — e o reinício é frequente, porque cada implantação
  derruba o processo (RNF-17).
