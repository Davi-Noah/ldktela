# Registro de decisões de implementação

Formato: `[Estágio] Nome curto — o que foi escolhido, e a alternativa descartada em meia linha.`

Este arquivo registra apenas o que a documentação normativa não decide, e as
divergências encontradas entre documentos normativos. Ele **não** altera a
especificação: conflito é registrado aqui e resolvido pela fonte de maior
autoridade (`docs/srs/` > `docs/protocol/` > `docs/api/` > `CLAUDE.md`).

## Divergências entre documentos e disco

- **[E0] Layout de `docs/` diverge do CLAUDE.md §1** — o CLAUDE.md aponta `docs/srs/`,
  `docs/protocol/websocket.md` e `docs/api/rest-api.md`; o disco tem
  `docs/SRS-v1.1-plataforma-comunicacao.md`, `docs/websocket.md` e `docs/rest-api.md`.
  Os arquivos não foram movidos nem editados (C6). Leia os caminhos reais; a tabela
  do CLAUDE.md §1 permanece válida por conteúdo, não por caminho.
- **[E0] Nome do arquivo de exemplo de ambiente** — o disco trazia `env.example`;
  CLAUDE.md §3 e o SRS chamam de `.env.example`. Renomeado para `.env.example`,
  conteúdo inalterado exceto pelo item seguinte.
- **[E0] `BACKUP_CRON` passa a ser aspeado em `.env.example`** — `BACKUP_CRON=0 4 * * *`
  quebra o parser de dotenv do `just` (espaços em valor não aspeado), impedindo
  qualquer receita de rodar. Valor e nome preservados: `BACKUP_CRON="0 4 * * *"`.
- **[E0] SRS §9 (F4a) diz 403 para não participante de DM; `docs/rest-api.md` §3 diz 404**
  para todo recurso invisível, incluindo canal e mensagem, com justificativa explícita
  de não vazamento de estrutura. Conflito registrado. Adotado **404**: a regra de
  vazamento do contrato REST é a norma específica e o SRS RF-18a não contradiz.
  Reavaliar se o SRS for revisado.

## Decisões

- **[E0] Versões da stack fixadas às mais recentes compatíveis** — `livekit-api` 0.6
  (não 0.4), `jsonwebtoken` 11 (não 9), `argon2` 0.6, `validator` 0.21, `reqwest` 0.13,
  `testcontainers` 0.28. O CLAUDE.md §5 fixa os *crates*, não os números de versão;
  as versões citadas lá já não existem como últimas.
- **[E0] `desktop/src-tauri` fora do workspace Cargo raiz** — evita que
  `cargo test --workspace` arraste a árvore inteira do Tauri. Em troca, `just lint`
  roda `cargo clippy` explicitamente sobre o manifesto do src-tauri, de modo que o
  núcleo nativo continua dentro do oráculo. Alternativa descartada: incluí-lo como
  membro, o que acopla todo `cargo test` ao WebView2.
- **[E0] `set windows-shell` no justfile aponta para o Git Bash** — no Windows, `bash`
  resolvido pelo PATH do sistema cai no relay do WSL (`System32\bash.exe`), que falha
  com `execvpe(/bin/bash)`. Alternativa descartada: reordenar o PATH da máquina.
- **[E0] `wait-db` usa `pg_isready` de dentro do container** — a máquina de
  desenvolvimento não precisa de cliente PostgreSQL instalado. `infra-up` usa
  `docker compose up -d --wait` com healthcheck, e `wait-db` fica como utilitário.
- **[E0] Faixa UDP do LiveKit em dev é 50000–50019** — mapear 10.000 portas no Docker
  Desktop é inviável. A faixa de produção (50000–60000, SRS §7.1) fica documentada no
  compose e não é usada localmente.
- **[E0] `/health` e `/metrics` sob `/api/v1`** — `docs/rest-api.md` §6.10 lista ambos na
  tabela cuja base é `/api/v1`; o aceite de F0 no SRS cita `curl https://.../health` sem
  prefixo. Adotado o prefixo da base, que é a norma mais específica.
- **[E0] `Config` vive em `crates/api/src/config.rs`** — é quem consome a maior parte das
  variáveis. `server` só a carrega e injeta. Alternativa descartada: um crate `config`
  próprio, que seria abstração antes de três usos (CLAUDE.md §2.10).
- **[E0] CI em dois jobs** — `check` completo em `ubuntu-latest` (único runner com Docker
  para testcontainers e serviço de Postgres) e `windows-compile` em `windows-latest`
  apenas com `cargo check`, para pegar quebra específica da plataforma de distribuição.
