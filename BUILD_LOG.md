# BUILD LOG

## Estado atual

Último estágio concluído: E2
Próximo estágio: E3
Portões vermelhos abertos: nenhum
Pendências de humano:
- Toolchain instalada por este agente nesta máquina: rustup 1.98.0, `just` 1.42.4,
  `sqlx-cli` 0.8.6 em `~/.cargo/bin` (adicionado ao PATH de usuário). Docker Desktop
  precisa estar rodando antes de `just infra-up`.
- Nenhuma credencial real existe. `.env` local usa valores `dev-only-not-a-real-*`
  para R2, LiveKit e Discord. Provisionamento de VM, R2, LiveKit de produção, TURN,
  DNS/TLS e token de bot do Discord permanecem pendentes (fora de escopo do agente).

## Notas de ambiente (leia ao retomar)

- `just` só funciona com Docker Desktop ativo para as receitas de infra.
- O PATH do shell do agente precisa de `export PATH="$HOME/.cargo/bin:$PATH"`.
- `just check` exige `.env` na raiz (gerado de `.env.example`) e Postgres de pé,
  porque `cargo sqlx prepare --check` compila o workspace com as macros do SQLx.

---

## E0 — Bootstrap · CONCLUÍDO

Portão: `just infra-up` (postgres + livekit healthy) e `just check` → `OK — tudo verde`.
Contagem: 0 testes Rust, 0 testes TS — não há lógica ainda; o portão do E0 é o próprio
pipeline de verificação existir e passar.

Entregue: workspace Cargo com os sete crates de CLAUDE.md §3 e a direção de dependência
correta · `rust-toolchain.toml` fixado em 1.98.0 · `docker/compose.dev.yml` com PostgreSQL 16
e LiveKit self-hosted, ambos com healthcheck · scaffold `desktop/` (Vite 6 + React 19 +
TS strict + Tailwind 4 + vitest 3 + eslint 9 flat + prettier) · scaffold `desktop/src-tauri`
(Tauri v2, plugins de atalho global e notificação, keyring) · endpoint real `GET /api/v1/health`
servido pelo binário `server` com tracing JSON · CI em GitHub Actions rodando `just check`.

Arquivos:
- raiz: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `.env.example` (renomeado de
  `env.example`), `justfile` (alterado), `BUILD_LOG.md`
- `crates/*/Cargo.toml` e `crates/*/src/lib.rs` (7 crates); `crates/api/src/{lib,config}.rs`,
  `crates/api/src/routes/{mod,health}.rs`; `crates/server/src/main.rs`; `crates/db/src/lib.rs`
- `docker/compose.dev.yml`, `docker/livekit.dev.yaml`
- `desktop/{package.json,vite.config.ts,tsconfig.json,eslint.config.js,.prettierrc.json,index.html}`,
  `desktop/src/{main.tsx,App.tsx,index.css,ui/tokens.css,test/setup.ts}`
- `desktop/src-tauri/{Cargo.toml,build.rs,tauri.conf.json,src/{main,lib}.rs,icons/*}`
- `.github/workflows/ci.yml`, `docs/DECISIONS.md`

Decisões registradas: 10 linhas em `docs/DECISIONS.md` (versões da stack, src-tauri fora do
workspace, `windows-shell` no justfile, `wait-db` via container, faixa UDP do LiveKit em dev,
`/health` sob `/api/v1`, local do `Config`, CI em dois jobs) e 4 divergências entre documentos
(layout de `docs/`, nome do `.env.example`, aspas em `BACKUP_CRON`, 403 vs 404 em DM).

Ressalva: `just check` inclui `cargo sqlx prepare --check`, que compila com as macros do SQLx.
Hoje não há nenhuma query, então `.sqlx/` não existe e a checagem é trivialmente verde. A partir
do E3 ela passa a exigir banco de pé — isso é o comportamento pretendido, não um defeito, mas
significa que `just check` nunca será executável sem Docker.

Ressalva 2: o CI nunca foi executado (não há remote configurado). O workflow está escrito
contra `ubuntu-latest` + `windows-latest` e não foi validado por execução real.

Pendente de humano: criar o repositório remoto e rodar o CI uma vez; instalar WebView2
Runtime se ausente (o app Tauri não abre sem ele — não verificado nesta máquina).

---

## E1 — Schema · CONCLUÍDO

Portão: `just db-reset` → 6 migrations aplicadas, sem erro. `cargo test -p db` → 4 testes
verdes (13,98 s, PostgreSQL 16 real via testcontainers). `just check` → `OK — tudo verde`.

Entregue: as 20 tabelas do SRS §5.2 transcritas literalmente (nomes, tipos, defaults,
constraints e comentários preservados) em seis migrations reversíveis · os três enums
(`channel_type`, `overwrite_target`, `message_origin`) · os índices parciais
(`idx_users_email`, `idx_users_username`, `idx_roles_default`, `idx_messages_channel`,
`idx_messages_pinned`, `idx_participants_user`, `idx_refresh_user`, `idx_outbox_pending`) ·
o índice GIN de busca em `to_tsvector('portuguese', content)` · `voice_states` UNLOGGED ·
`pg_trgm` criada com o índice de trigrama comentado (P-02, desativado por padrão) ·
fixture `TestDb` para os estágios seguintes.

Arquivos:
- `migrations/000{1..6}_{extensions,identity,structure,messages,voice,bridge}.{up,down}.sql`
- `crates/db/tests/{migrations.rs,common/mod.rs}`, `crates/db/Cargo.toml`

Testes que expressam o critério: `migrations_apply_revert_and_reapply_leaving_no_residue`
(aplica, compara o conjunto de tabelas e enums contra a lista do SRS, reverte tudo com
`Migrator::undo(…, 0)`, prova resíduo zero, reaplica) ·
`schema_enforces_the_srs_constraints_that_carry_meaning` (chk_real_user_credentials,
username único só para conta real, chk_channel_scope nos dois sentidos, chk_bridge_scope
em DM, idx_roles_default) · `voice_states_is_unlogged_as_the_srs_requires` ·
`full_text_index_uses_the_portuguese_configuration`.

Decisões registradas: 3 linhas (testcontainers 0.27 corrigindo o E0; migrations fatiadas
por bloco do SRS; o schema normativo tem 20 tabelas e não as 17 citadas no changelog C-07).

Ressalva: `invites.guild_id` não tem chave estrangeira no SRS §5.2 e foi transcrito assim.
É provável lapso da especificação — um convite pode apontar para um guild inexistente.
Não corrigido (C6); registrado aqui.

Ressalva 2: `mentions` não tem chave primária no SRS §5.2, o que permite linhas duplicadas
idênticas. Transcrito literalmente. A extração de menções do E7 precisa deduplicar na
aplicação.

Pendente de humano: nenhum.

---

## E2 — Contratos · CONCLUÍDO

Portão: `just types` → 87 arquivos `.ts` em `desktop/src/api/types/`, `tsc --noEmit` limpo.
`cargo test -p domain` → 25 testes verdes; `cargo test -p protocol` → 106 (dos quais 60 são
os `export_bindings_*` do ts-rs). `just check` → `OK — tudo verde`.

Entregue: `domain::permissions` com os 20 bits do SRS §5.3, máscara em `i64`, truncamento de
bits reservados e formato decimal em string · `domain::resolve` com os oito passos transcritos
literalmente, incluindo a acumulação de `role_allow`/`role_deny` antes da aplicação (passo 6) ·
`domain::validation` com os limites derivados das larguras de coluna · `protocol` com todos os
DTOs de REST e WS derivando ts-rs, os scalars de wire (Timestamp RFC 3339, PermissionMask e
Snowflake como string decimal), o envelope do gateway (`DispatchFrame` achatado sobre
`DispatchEvent` adjacente) e os 30 eventos de dispatch.

Arquivos:
- `crates/domain/src/{lib,permissions,resolve,validation}.rs`
- `crates/protocol/src/{lib,scalars,patch,error,page,user,auth,guild,channel,message,search,voice,bridge,gateway}.rs`
- `.cargo/config.toml` (destino do ts-rs), `desktop/src/api/types/*.ts` (gerados, versionados)

Testes que expressam o critério (todos em `crates/domain/src/resolve.rs`):
`step0_active_participant_of_a_dm_gets_the_fixed_grant`, `step0_non_participant_of_a_dm_gets_nothing`,
`step0_short_circuits_before_roles_and_overwrites`, `step1_guild_owner_gets_everything_even_with_everything_denied`,
`step4_administrator_beats_every_channel_overwrite`, `step4_administrator_from_everyone_role_also_short_circuits`,
`step5_private_channel_is_a_deny_of_view_channel_on_everyone`, `step6_role_allow_reopens_a_channel_closed_for_everyone`,
`step6_role_overwrites_accumulate_before_being_applied` (prova que a ordem das linhas não muda o
resultado), `step7_member_deny_overrides_a_role_allow`, `step7_member_allow_overrides_a_role_deny`.

Decisões registradas: 10 linhas (ALL como união dos 20 bits definidos; truncamento de bits
desconhecidos; scalars de wire; i64/u64 exportados como `number`; `Option<Option<T>>` literal;
validação em `domain` sem o crate `validator`; limites não especificados; `READY` com
`categories` e `members`; `muted` em `READ_STATE_UPDATE`; dois códigos de erro novos).

Ressalva: `Permissions::ALL` vale 1048575, e é esse número que sai em `channel.permissions`
para um owner ou administrador. Se um bit for adicionado ao SRS §5.3, o valor muda — é
comportamento pretendido, mas qualquer teste de cliente que fixe a constante vai quebrar.

Pendente de humano: nenhum.
