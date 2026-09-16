set dotenv-load := true
set shell := ["bash", "-uc"]
# No Windows, `bash` no PATH do sistema resolve para o relay do WSL (System32),
# que nao tem /bin/bash. Apontar explicitamente para o Git Bash, ja exigido pelo repo.
set windows-shell := ["C:/Program Files/Git/bin/bash.exe", "-uc"]

# Lista as receitas disponiveis
default:
    @just --list

# ---------------------------------------------------------------------------
# Verificacao. E o oraculo de correcao do projeto: se falha, nada esta pronto.
# ---------------------------------------------------------------------------
check: fmt-check lint sqlx-check test-rs test-tauri types-check test-ts
    @echo "OK — tudo verde"

fmt:
    cargo fmt --all
    cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --all
    cd desktop && npm run format

fmt-check:
    cargo fmt --all -- --check
    cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --all -- --check
    cd desktop && npm run format:check

# O `cd` nao e estilo: o cargo le .cargo/config.toml pelo diretorio ATUAL, e nao
# pelo --manifest-path. De fora, o crt-static que o libwebrtc exige nao seria
# aplicado e o link falharia com centenas de LNK2038. Ver ADR-0026.
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cd desktop/src-tauri && cargo clippy --all-targets -- -D warnings
    cd desktop && npm run lint

# Falha se .sqlx/ estiver desatualizado em relacao as queries do codigo
sqlx-check:
    cargo sqlx prepare --workspace --check -- --all-targets

types-check:
    cd desktop && npx tsc --noEmit

test-rs:
    cargo test --workspace

# O core do cliente esta fora do workspace, entao `cargo test --workspace` nao o
# alcanca: sem esta receita, a captura de tela e o audio ficariam sem teste
# nenhum rodando.
test-tauri:
    cd desktop/src-tauri && cargo test

test-ts:
    cd desktop && npx vitest run

# ---------------------------------------------------------------------------
# Banco de dados
# ---------------------------------------------------------------------------

# Sobe Postgres + LiveKit locais
infra-up:
    docker compose -f docker/compose.dev.yml up -d --wait

infra-down:
    docker compose -f docker/compose.dev.yml down

# Aguarda o Postgres aceitar conexoes. Usa o cliente de dentro do container:
# a maquina de desenvolvimento nao precisa de psql instalado.
wait-db:
    @until docker compose -f docker/compose.dev.yml exec -T postgres pg_isready -U comms -d comms >/dev/null 2>&1; do sleep 0.5; done

migrate:
    sqlx migrate run

migrate-revert:
    sqlx migrate revert

# Cria uma migration nova: just migration add_read_states
migration name:
    sqlx migrate add -r {{name}}

# Regenera o cache offline do SQLx. RODE SEMPRE QUE ALTERAR UMA QUERY.
prepare:
    cargo sqlx prepare --workspace -- --all-targets
    @echo "Lembre de versionar .sqlx/"

# Recria o banco do zero e aplica todas as migrations
db-reset:
    sqlx database drop -y
    sqlx database create
    just migrate

# ---------------------------------------------------------------------------
# Desenvolvimento
# ---------------------------------------------------------------------------

# Sobe a infra e roda o servidor. Recarga automatica so se o cargo-watch existir:
# exigir uma ferramenta nao instalada para o comando principal faz o projeto
# parecer quebrado quando o que falta e uma conveniencia.
dev: infra-up migrate
    @if command -v cargo-watch >/dev/null 2>&1; then \
        cargo watch -x 'run -p server'; \
    else \
        echo ">> cargo-watch nao encontrado: rodando sem recarga automatica."; \
        echo ">> Para ter recarga ao salvar: just install-tools"; \
        cargo run -p server; \
    fi

# Roda o servidor sem infra e sem watch. Util quando o Postgres ja esta de pe.
serve:
    cargo run -p server

# Ferramentas opcionais de desenvolvimento.
install-tools:
    cargo install cargo-watch

app:
    cd desktop && npm run tauri dev

# Gera os tipos TypeScript a partir do crate protocol (ts-rs)
types:
    cargo test -p protocol export_bindings
    @echo "Tipos gerados em desktop/src/api/types/"

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

build-server:
    cargo build --release -p server

build-app:
    cd desktop && npm run tauri build
