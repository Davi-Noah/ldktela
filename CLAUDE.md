# CLAUDE.md

Instruções de trabalho para este repositório. Leia por inteiro antes da primeira alteração de cada sessão.

## 1. O que é este projeto

Plataforma privada de comunicação em tempo real para desktop (Windows), substituindo um servidor do Discord para uma comunidade de 10 a 30 pessoas. Chat de texto, conversas diretas, busca, voz, vídeo, compartilhamento de tela com áudio, migração do histórico e ponte bidirecional permanente com o Discord.

Especificação normativa: `docs/srs/`. Este arquivo não repete o SRS — ele diz **como se trabalha aqui**.

| Preciso de… | Leia |
|---|---|
| Requisitos, schema, permissões, riscos | `docs/srs/` |
| Formato de evento em tempo real, resume, fan-out | `docs/protocol/websocket.md` |
| Endpoints, erros, paginação, autenticação | `docs/api/rest-api.md` |

**Não carregue o SRS inteiro para tarefas pontuais.** Abra o arquivo da seção relevante.

## 2. Regras não negociáveis

Violar qualquer uma destas invalida o trabalho, mesmo que compile e passe nos testes.

1. **Nunca use `OFFSET` para paginar.** Toda paginação é por keyset sobre `(channel_id, id)`, com IDs UUIDv7. Ver §5.1 do SRS.
2. **Nunca gere IDs no banco.** IDs são criados na aplicação com `Uuid::now_v7()`. O schema não tem `DEFAULT` para chave primária.
3. **Nunca use `unwrap`, `expect` ou `panic!` em código de request.** Só em testes e em inicialização do processo (onde falhar cedo é correto).
4. **Nunca use `anyhow` em handler.** Todo erro que chega ao cliente passa pelo `AppError` (§6).
5. **Nunca troque `sqlx::query!` por `sqlx::query`** para fazer o build passar. Se a macro falha, o cache offline está desatualizado: rode `just prepare`.
6. **Nunca use `localStorage` ou `sessionStorage`** para tokens ou dados do usuário. Refresh token vive no cofre do sistema, via core Rust. Estado de UI vive em memória.
7. **Toda consulta que retorna conteúdo de canal filtra por `VIEW_CHANNEL` no momento da consulta.** Nunca confie em cache de permissão. Vale para REST, WebSocket e busca.
8. **Nunca faça fan-out de evento para um guild inteiro.** O conjunto de destinatários é sempre calculado por canal. Ver §4 do protocolo WebSocket.
9. **Não instale dependência nova sem justificar** em uma linha no PR. Preferir o que já está no `Cargo.toml`/`package.json`.
10. **Não crie abstração antes de três usos reais.** Trait com uma implementação, "service layer" que só repassa e mock de repositório são proibidos: os testes usam Postgres de verdade.

## 3. Layout do workspace

```
.
├── CLAUDE.md
├── justfile
├── .env.example
├── rust-toolchain.toml
├── docs/
│   ├── srs/                    # especificacao fatiada por modulo
│   ├── protocol/websocket.md
│   └── api/rest-api.md
├── migrations/                 # SQL versionado (sqlx migrate)
├── .sqlx/                      # cache offline do SQLx — VERSIONADO
├── crates/
│   ├── protocol/               # DTOs de wire (REST + WS). Zero logica. Gera TS via ts-rs.
│   ├── domain/                 # regras puras: permissoes, validacoes. Sem IO, sem sqlx, sem axum.
│   ├── db/                     # SQLx: pool, repositorios, migrations embarcadas.
│   ├── api/                    # Axum: rotas REST, gateway WS, middleware de auth.
│   ├── bridge/                 # Bot do Discord, worker do outbox, reconciliacao.
│   ├── migrator/               # CLI de ingestao do export (binario separado).
│   └── server/                 # binario unico que compoe api + bridge.
├── desktop/
│   ├── src-tauri/              # core Rust: tray, atalho global, cofre, IPC
│   └── src/                    # React 19 + TS
│       ├── api/                # cliente REST tipado (tipos gerados)
│       ├── gateway/            # cliente WS, resume, dispatch
│       ├── store/              # zustand: estado normalizado
│       ├── features/           # chat/, voice/, dms/, search/, settings/
│       └── ui/                 # componentes de base
└── spike/                      # DESCARTAVEL. Nao importar daqui. Ver §9.
```

**Direção de dependência, obrigatória:** `protocol` e `domain` não dependem de ninguém. `db` depende de `domain` e `protocol`. `api` e `bridge` dependem de `db`. `server` depende de tudo. Nenhuma seta na direção contrária, nunca.

`domain` não pode importar `sqlx`, `axum` nem `tokio`. Se você precisou, o código está no crate errado.

## 4. Comandos

```bash
just check      # OBRIGATORIO antes de considerar qualquer tarefa concluida
just dev        # sobe Postgres + LiveKit em docker, roda o server em watch
just migrate    # aplica migrations
just prepare    # regenera .sqlx/ (rode SEMPRE que alterar uma query)
just types      # regenera os tipos TypeScript a partir do crate protocol
just test       # testes com Postgres real via testcontainers
just app        # roda o app Tauri em modo dev
```

`just check` roda: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo sqlx prepare --check`, `cargo test`, `tsc --noEmit`, `vitest run`, `eslint`. **Se `just check` falha, a tarefa não está pronta.** Não relate conclusão com o comando vermelho.

## 5. Stack fixada

Não substitua estas escolhas sem discussão explícita.

**Rust:** axum 0.8 · tokio · sqlx 0.8 (postgres, macros, uuid, time) · serde · thiserror · tracing + tracing-subscriber (JSON) · argon2 · jsonwebtoken · uuid (feature `v7`) · tower-http (cors, trace, limit) · validator · aws-sdk-s3 (endpoint customizado para R2) · livekit-api (emissão de token e verificação de webhook) · serenity (Discord).

> Serenity foi escolhido em vez de twilight deliberadamente: tem muito mais exemplos públicos, o que melhora a qualidade do código gerado por agente. É a escolha certa dado o modo de desenvolvimento deste projeto, não necessariamente a mais elegante.

**Frontend:** React 19 · TypeScript strict · Vite · Tailwind · zustand (estado de domínio) · TanStack Query (REST) · @tanstack/react-virtual (lista de mensagens) · livekit-client · marked (lexer) · shiki (carregado sob demanda).

**Proibido no frontend:** qualquer biblioteca de componentes pesada, qualquer state manager além de zustand + Query, `react-markdown` (custo de render incompatível com o RNF-04).

## 6. Taxonomia de erro

Um único enum, em `crates/api/src/error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unauthorized")]            Unauthorized,
    #[error("forbidden")]               Forbidden,
    #[error("not found")]               NotFound { resource: &'static str },
    #[error("validation failed")]       Validation(Vec<FieldError>),
    #[error("conflict")]                Conflict { reason: &'static str },
    #[error("rate limited")]            RateLimited { retry_after_ms: u64 },
    #[error("payload too large")]       PayloadTooLarge,
    #[error("upstream failure")]        Upstream(#[from] UpstreamError),
    #[error("internal")]                Internal(#[from] anyhow::Error),
}
```

Regras:
- `AppError` implementa `IntoResponse` e é o **único** tipo de erro em assinatura de handler: `Result<Json<T>, AppError>`.
- `Internal` nunca vaza detalhe ao cliente. A mensagem real vai para o log com o `request_id`; a resposta traz só o código e o `request_id`.
- `Forbidden` e `NotFound` para recurso invisível: se o usuário não tem `VIEW_CHANNEL`, responda **404**, não 403. 403 confirma existência.
- Formato do corpo de erro: ver §3 de `docs/api/rest-api.md`. Não invente outro.

## 7. Convenções de código

**Rust**
- Handlers finos: extraem, validam, chamam `domain`/`db`, mapeiam para DTO. Nenhuma regra de negócio em handler.
- Todo handler tem `#[tracing::instrument(skip(state, ...))]` com o `request_id` no span.
- Queries em `crates/db/src/repo/*.rs`, nunca inline em handler.
- Transações explícitas onde há mais de uma escrita relacionada. Migração, vinculação de identidade e criação de DM são transacionais por definição.
- Testes de integração usam Postgres real (`testcontainers`). Não escreva mock de repositório.

**TypeScript / React**
- `strict: true`. `any` proibido; use `unknown` e refine.
- Tipos de payload **nunca escritos à mão**: vêm de `just types`. Se falta um tipo, adicione no crate `protocol` e regenere.
- Estado de domínio (mensagens, canais, membros, presença) em zustand, **normalizado por ID**. TanStack Query só para busca inicial e paginação; eventos do WS escrevem direto no store.
- Fonte da verdade da lista de mensagens é o store, não o cache do Query. O WS é a fonte de atualização; o REST é a fonte de recuperação de lacuna.
- Envio otimista: o cliente gera um `nonce`, insere a mensagem localmente com estado `pending`, e reconcilia quando o `MESSAGE_CREATE` volta com o mesmo `nonce`. Timeout de 10 s marca `failed` com opção de reenviar.
- Markdown: parse com o lexer do `marked` **uma vez por mensagem**, com cache por `message.id`. O componente renderiza tokens; não re-parseia em cada render. Isto é requisito de performance (RNF-04), não estilo.
- Imagens sempre renderizadas com `width`/`height` vindos do banco, para não causar reflow durante scroll.

## 8. Direção visual (provisória)

Suficiente para não sair um visual de template; revisão de design vem depois.

- Tema escuro como padrão e único na v1. Superfícies em cinzas neutros frios, não em preto puro.
- Densidade alta: a lista de mensagens é o produto. Espaçamento vertical apertado, agrupamento de mensagens consecutivas do mesmo autor em até 5 minutos.
- Uma cor de destaque só, usada para foco, menção e estado ativo. Cores de cargo aparecem apenas no nome do autor.
- Tipografia: uma sans para UI, uma mono para código. Tamanho base 14px, altura de linha 1.45.
- Sem sombras difusas, sem gradientes decorativos, sem animação em elemento de lista.
- Tokens em `desktop/src/ui/tokens.css`, consumidos via Tailwind. Nenhum valor de cor escrito direto em componente.

## 9. O diretório `spike/`

Contém a prova de conceito de compartilhamento de tela (fatia F1). É **descartável e proibido de importar**. Ele existe só para registrar números medidos (bitrate real, egress, latência glass-to-glass, CPU de quem compartilha) em `spike/RESULTS.md`. Quando F6 começar, o código de produção é escrito do zero.

## 10. Zonas que exigem revisão humana

Não altere sem confirmação explícita:

- `migrations/` — qualquer migration destrutiva (drop, alteração de tipo, remoção de coluna).
- Configuração de rede e infraestrutura da VM (firewall, portas do LiveKit, TLS).
- Código de captura de mídia e permissões de sistema operacional em `desktop/src-tauri/`.
- Qualquer coisa que toque em credencial, token de webhook ou chave de assinatura.
- A ordem de resolução de permissão (§5.3 do SRS). Se parecer errada, pergunte; não "corrija".

## 11. Como conduzir uma tarefa

1. Identifique a fatia do roadmap (F0–F8) a que a tarefa pertence. Se não pertence a nenhuma, pergunte antes de escrever código.
2. Leia os documentos relevantes da tabela em §1.
3. Escreva o teste que expressa o critério de aceite da fatia **antes** da implementação.
4. Implemente o caminho mais direto que passa no teste. Sem generalizar para casos futuros.
5. Rode `just check`.
6. Relate: o que mudou, quais arquivos, o que ficou de fora e por quê.

**Não faça refatoração fora do escopo da tarefa.** Se encontrar algo errado em outro lugar, registre em uma linha no relato em vez de consertar.

## 12. Definição de pronto

- `just check` passa inteiro.
- O critério de aceite da fatia tem um teste com nome que o descreve.
- Nenhum `TODO` novo sem issue associada.
- Se a tarefa tocou em query: `.sqlx/` regenerado e versionado.
- Se a tarefa tocou em DTO: tipos TypeScript regenerados e versionados.

## 13. Idioma

**DECIDIDO** — inverta aqui, num lugar só, se discordar:

- Identificadores, nomes de arquivo e comentários de código: **inglês**.
- Documentação (`docs/`, este arquivo), textos de interface e conteúdo do produto: **português do Brasil**.
- Mensagens de commit: **inglês**, Conventional Commits (`feat:`, `fix:`, `chore:`).
- Nomes de eventos e campos de wire: **inglês**, `SCREAMING_SNAKE_CASE` para eventos, `snake_case` para campos.
