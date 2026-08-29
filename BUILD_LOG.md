# BUILD LOG

## Estado atual

Último estágio concluído: E5
Próximo estágio: E6
Portões vermelhos abertos: nenhum
Carregado para estágios seguintes:
- E6 precisa emitir `PERMISSIONS_STALE` e invalidar o índice de fan-out nas mutações do E5
  (contrato §6.4). O E5 entregou as rotas sem o dispatch, porque o gateway ainda não existe.
- O portão do E5 cita canal, **mensagem** e **anexo**. As rotas de mensagem (E7) e de anexo
  (E8) ainda não existem; as duas asserções de 404 correspondentes ficam devendo e serão
  escritas nos respectivos estágios.
Pendências de humano:
- **Limite de taxa ainda não existe.** `POST /auth/login` e `/auth/register` estão sem os
  5/min por IP e por conta do contrato REST §5. O RNF-06 conta com esse limite como parte da
  defesa. Está atribuído ao E16; até lá, não expor o servidor à internet.
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

---

## E3 — Persistência · CONCLUÍDO

Portão: `cargo test -p db` → 26 testes verdes contra PostgreSQL 16 real
(4 migrations + 4 keyset + 10 permissões + 8 transações), 12,5 s no total.
`just prepare` → 44 entradas em `.sqlx/`, versionadas. `just check` → `OK — tudo verde`.

Entregue: `DbError` com deteção de violação de constraint · enums `channel_type`,
`overwrite_target` e `message_origin` mapeados com `sqlx::Type` no crate `db`, com conversão
para os equivalentes de wire · repositórios de `users`, `invites`, `refresh_tokens`,
`channels` (inclusive participantes de DM e resolução de 1:1 existente) e `messages` ·
paginação por keyset com os quatro cursores do contrato (`latest`, `before`, `after`,
`around`), sempre sobre `(channel_id, id)`, com `has_more` obtido lendo uma linha a mais ·
`repo::permissions` montando o contexto do SRS §5.3 a partir do banco em três consultas e
delegando o algoritmo a `domain::resolve`.

Arquivos:
- `crates/db/src/{lib,error,types}.rs`
- `crates/db/src/repo/{mod,users,invites,refresh_tokens,channels,messages,permissions}.rs`
- `crates/db/tests/{common/mod,keyset,permissions,transactions}.rs`
- `.sqlx/` (44 arquivos)

Teste do portão: `keyset_pagination_is_stable_under_concurrent_inserts` — 200 mensagens
pré-existentes, quatro tarefas inserindo durante toda a varredura, paginação para trás em
páginas de 25. Prova três propriedades: nenhum id repetido, todas as 200 linhas
pré-existentes presentes, e ordem estritamente decrescente dentro e entre páginas. O teste
falha o assert de concorrência se nenhuma escrita concorrente tiver ocorrido, para não passar
por acidente. Complementado por `uuid_v7_is_strictly_increasing_even_when_generated_concurrently`
(20.000 ids em 4 threads, zero colisões), que é a premissa de que o keyset depende.

Decisões registradas: 7 linhas (container compartilhado com banco por teste; `Option<Permissions>`
distinguindo inexistente de invisível; não-membro e banido resolvem para NONE; `resolve_for_guild`
separado; overwrites lidos numa consulta; nonce fora do schema; `NewGuildChannel`).

Ressalva: repositórios de guilds, categorias, cargos, membros, overwrites, anexos, reações,
estado de leitura, voz e ponte ainda não existem — entram nos estágios que os consomem
(E5, E7, E8, E11, E15). O E3 entregou a fundação e os repositórios exigidos pelos portões
de E3 e E4.

Ressalva 2: `resolve_for_channel` faz três round trips por chamada e roda em toda consulta
que retorna conteúdo (CLAUDE.md §2.7). Com 30 usuários não é gargalo; se virar, o caminho é
uma única consulta com CTEs, não um cache.

Pendente de humano: nenhum.

---

## E4 — Autenticação · CONCLUÍDO

Portão: `cargo test -p api` → 36 testes verdes (23 unitários + 13 de integração HTTP contra
PostgreSQL real), 10,1 s. `just check` → `OK — tudo verde`.

Entregue: Argon2id com os parâmetros do RNF-06, recusados na partida se estiverem abaixo ·
access token JWT HS256 de 15 min com `sub`, `jti`, `iat`, `exp` · refresh opaco de 256 bits
armazenado como SHA-256, rotativo, com família e detecção de reúso · `AppError` com
`IntoResponse` e o formato de erro único do §3, incluindo `Retry-After` em 429 ·
middleware de `request_id` propagado para o span de tracing, para o cabeçalho e para o corpo
de erro · `POST /auth/{register,login,refresh,logout}` · `GET/PATCH /users/@me`,
`GET /users/{id}`.

Arquivos:
- `crates/api/src/{lib,config,error,state}.rs`
- `crates/api/src/auth/{mod,password,token,session}.rs`
- `crates/api/src/middleware/{mod,auth,request_id}.rs`
- `crates/api/src/routes/{mod,auth,users,health}.rs`
- `crates/api/tests/{common/mod,auth}.rs`, `crates/server/src/main.rs`

Teste do portão: `reusing_a_consumed_refresh_token_revokes_the_entire_family` — gira o token
três vezes, reapresenta o primeiro (já consumido), prova 401 `TOKEN_REUSED`, e prova que o
terceiro token — que estava vivo — também deixou de funcionar. Conta as linhas no banco:
três emitidas, zero utilizáveis. Complementado por
`two_simultaneous_refreshes_with_the_same_token_kill_the_family` (corrida real com
`tokio::join!`) e `revoking_one_family_leaves_the_other_sessions_of_the_same_user_alive`.

Decisões registradas: 8 linhas (argon2 0.5 e jsonwebtoken com `rust_crypto`; SHA-256 no
refresh; perdedor da corrida derruba a família; `verify_dummy` no login; validação de
configuração na partida; sanitização do `X-Request-Id`; fixtures sem pool compartilhado;
perfis visíveis entre membros).

**Número medido — RNF-06.** `argon2id verify (m=65536, t=3, p=4)`, build de release, nesta
máquina (x86_64 desktop): **167 ms por verificação**. Em debug: 3,02 s — relevante porque
os testes de integração usariam esse custo se não usassem parâmetros baratos. O SRS estima
~100 ms na VM ARM de 2 OCPU; o número real dela ainda não foi medido. O crate `argon2` roda
as 4 lanes sequencialmente sem a feature `parallel` (rayon), então o custo em relógio é
maior do que uma implementação paralela daria.

Ressalva: **não há limite de taxa em nenhuma rota.** O contrato §5 exige 5/min em login e
registro, por IP e por conta, e o RNF-06 conta com isso. Está no E16; registrado como
pendência aberta no topo deste arquivo.

Ressalva 2: `GET /users/@me` reporta `status: "offline"` sempre, porque presença vive no
gateway e o gateway é do E6. Não é stub — é o valor correto até existir sessão de WebSocket.

Pendente de humano: medir o custo do Argon2id na VM ARM real antes de produção.

---

## E5 — Estrutura · CONCLUÍDO

Portão: `cargo test -p api` → 53 testes verdes (23 unitários + 13 de auth + 17 de estrutura).
`just check` → `OK — tudo verde`.

Entregue: CRUD de guilds, categorias, canais, cargos, membros e overwrites, com a permissão
exigida por rota do contrato §6.2–§6.4 · guardas `channel_visible`/`require_channel` e
`guild_visible`/`require_guild`, que separam "posso ver?" de "posso agir?" e nunca colapsam
as duas · convites ligados a guild, com o cadastro inserindo a conta nova em `guild_members` ·
extractors `Json`/`Path` próprios, para que corpo malformado e id malformado saiam no formato
de erro único · `clamp_to_own`, impedindo escalada por `MANAGE_ROLES` · subcomando
`server bootstrap` criando o primeiro guild.

Arquivos:
- `crates/db/src/repo/{guilds,roles,categories}.rs`, `crates/db/src/repo/channels.rs` (update,
  delete, reorder), `crates/db/src/repo/mod.rs`
- `crates/api/src/{permissions,extract}.rs`, `crates/api/src/routes/{guilds,channels,invites}.rs`,
  `crates/api/src/routes/auth.rs` (entrada no guild pelo convite)
- `crates/server/src/main.rs` (subcomando `bootstrap`)
- `crates/api/tests/structure.rs`, `crates/api/tests/common/mod.rs`

Teste do portão: `a_channel_denied_by_overwrite_answers_404_not_403` — um canal aberto e um
canal privado no mesmo guild; o membro recebe **403** no aberto (visível, ação negada) e
**404** no privado, e a mensagem do 404 é byte a byte igual à de um id inexistente.
`deleting_and_overwriting_an_invisible_channel_answer_404_too` cobre as outras três rotas de
canal. `a_guild_the_caller_does_not_belong_to_answers_404` e
`a_member_with_no_visible_channel_cannot_see_the_guild_either` cobrem o nível de guild.

Decisões registradas: 9 linhas, incluindo uma **lacuna de especificação**: o contrato REST não
tem endpoint de criação de guild. Resolvido fora do wire, com subcomando de CLI.

Ressalva: o portão do E5 pede a prova de 404 também em **mensagem** e **anexo**. Essas rotas
pertencem a E7 e E8 e ainda não existem, então essas duas asserções não foram escritas. Está
registrado no topo deste arquivo como item carregado.

Ressalva 2: nenhuma mutação de E5 emite evento de gateway. O contrato §6.4 exige
`PERMISSIONS_STALE` em toda alteração de cargo ou overwrite. Entra no E6 junto com o
barramento de eventos.

Ressalva 3: o defeito do 422 foi encontrado pelo teste de máscara-como-número, não previsto no
plano: todo corpo com forma errada estava escapando do formato de erro do §3. Corrigido na
origem com extractors próprios, o que afeta todas as rotas, inclusive as de E4.

Pendente de humano: nenhum.
