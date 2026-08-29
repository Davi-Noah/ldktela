# BUILD LOG

## Estado atual

Último estágio concluído: E0
Próximo estágio: E1
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
