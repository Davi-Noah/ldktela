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

- **[E1] `testcontainers` fixado em 0.27, não 0.28** — `testcontainers-modules` 0.15
  ainda exige `testcontainers ^0.27`; as duas versões conflitam em `bollard`.
  Corrige a linha registrada no E0.
- **[E1] Migrations em seis arquivos reversíveis por bloco do SRS §5.2** — extensions,
  identity, structure, messages, voice, bridge. Alternativa descartada: um arquivo único,
  que impede reverter parcialmente e torna o `down` uma bomba.
- **[E1] O schema tem 20 tabelas, não 17** — o changelog C-07 do SRS fala em 17, mas o
  bloco normativo §5.2 (com C-11 a C-14 aplicados) define 20. Adotado o §5.2, que é o
  texto normativo. Sem alteração na especificação.

- **[E2] `Permissions::ALL` é a união dos 20 bits definidos, não `i64::MAX`** — o SRS §5.3
  diz "todas as permissões" e reserva os bits 20..62 para expansão futura. Conceder bits
  reservados faria `ADMINISTRATOR` herdar automaticamente permissões que este build não
  sabe verificar. Alternativa descartada: 63 bits ligados.
- **[E2] Bits desconhecidos lidos do banco são descartados** (`from_bits_truncate`) — uma
  linha gravada por uma versão futura não concede permissão que esta versão não conhece.
- **[E2] Máscara, snowflake e timestamp são newtypes em `protocol::scalars`** — máscara e
  snowflake serializam como string decimal (rest-api §6.4); timestamp como RFC 3339, que
  não é o formato padrão do `time::OffsetDateTime`. Um lugar só, em vez de
  `#[serde(with = ...)]` em cada campo.
- **[E2] Todo campo `i64`/`u64` restante é exportado como `number` em TS** — o padrão do
  ts-rs é `bigint`, e `JSON.parse` nunca produz `bigint`. Os campos genuinamente grandes já
  são string. Alternativa descartada: `bigint` no cliente, que quebraria em toda aritmética.
- **[E2] `Patch<T>` escrito como `Option<Option<T>>` nos DTOs** — o ts-rs não enxerga através
  do alias e recusa `#[ts(optional)]`. O alias continua existindo em `protocol::patch` para
  leitura; os campos usam o tipo literal.
- **[E2] Validação fica em `domain`, sem usar o crate `validator`** — o derive do `validator`
  exigiria atributos nos DTOs de `protocol`, que é declarado "zero lógica" no CLAUDE.md §3.
  Manter as regras em `domain` evita dois caminhos de erro. O `validator` permanece
  declarado no workspace, não removido da stack.
- **[E2] Limites não especificados** — `MESSAGE_CONTENT_MAX = 4000` (a coluna é `TEXT`, sem
  limite; o wire precisa de um), `PASSWORD_MIN = 8`, `USERNAME` 2..32 em
  `[A-Za-z0-9._-]` com ao menos um alfanumérico, `SEARCH_QUERY` 2..200.
- **[E2] `READY.guilds[]` inclui `categories` e `members`** — o §3.1 do protocolo descreve
  READY como portador da estrutura, citando "membros" no texto; o exemplo JSON está
  elidido. Campos aditivos não incrementam a versão do protocolo (§8).
- **[E2] `READ_STATE_UPDATE` inclui `muted`** — campo aditivo sobre os três documentados,
  necessário para o cliente não recalcular estado de silenciamento.
- **[E2] Dois códigos de erro novos: `BRIDGE_NOT_ALLOWED` e `UPSTREAM_FAILURE`** — o §6.10
  do contrato REST exige 409 ao habilitar ponte em DM sem nomear o código, e o `AppError`
  do CLAUDE.md §6 tem a variante `Upstream` sem código correspondente na tabela §3.
