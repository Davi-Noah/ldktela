# CLAUDE.md

Instruções de trabalho para este repositório. Leia por inteiro antes da primeira alteração de cada sessão.

## 1. O que é este projeto

**Complemento ao Discord para compartilhamento de tela**, desktop (Windows). Voltado a comunidades que continuam vivendo no Discord, em regiões onde o compartilhamento de tela do Discord é bloqueado por lei ou degradado a ponto de inutilidade.

O Discord continua sendo a camada social: identidade, texto, voz, comunidade, permissões. Nós somos o plano de mídia da tela, e nada além disso.

> **Este projeto já foi outra coisa.** Até 2026-09-12 era uma plataforma completa que substituiria o Discord (chat, DMs, busca, voz, migração, ponte). O escopo antigo está morto — ver [ADR-0008](docs/adr/0008-complemento-ao-discord.md). Boa parte do código no disco ainda é da v1 e está marcada para remoção na fatia S1. Não construa em cima do que está marcado para sair.

| Preciso de… | Leia |
|---|---|
| Por que uma decisão é como é; o que já foi rejeitado | `docs/adr/` — comece pelo `README.md` |
| Requisitos, schema, RNFs, riscos | `docs/SRS-v2.0-complemento-screen-share.md` |
| Fatias, ordem, critérios de aceite | `docs/ROADMAP.md` |
| Formato de evento em tempo real, resume, fan-out | `docs/websocket.md` |
| Endpoints, erros, paginação, autenticação | `docs/rest-api.md` |
| Notas de implementação por estágio | `docs/DECISIONS.md` |

**Não carregue o SRS inteiro para tarefas pontuais.** Abra a seção relevante. Antes de propor qualquer recurso, leia o índice de ADRs: a rejeição pode já estar escrita.

## 2. Regras não negociáveis

Violar qualquer uma destas invalida o trabalho, mesmo que compile e passe nos testes.

1. **Se o Discord já faz, não reimplemente — integre.** É a regra de escopo do produto. Chat, anexos, busca, DMs, microfone, câmera e cargos próprios estão fora, com rejeição registrada em `docs/adr/`. Reabrir qualquer um exige ADR novo que substitua o anterior, nunca uma issue ou um "só um pouquinho".
2. **Toda decisão que restrinja trabalho futuro vira um ADR** em `docs/adr/NNNN-titulo.md`, antes de a tarefa ser dada como pronta. Ver [ADR-0007](docs/adr/0007-governanca-de-decisoes.md).
3. **Nunca use `OFFSET` para paginar.** Paginação é sempre por keyset, com IDs UUIDv7.
4. **Nunca gere IDs no banco.** IDs são criados na aplicação com `Uuid::now_v7()`. O schema não tem `DEFAULT` para chave primária.
5. **Nunca use `unwrap`, `expect` ou `panic!` em código de request.** Só em testes e em inicialização do processo (onde falhar cedo é correto).
6. **Nunca use `anyhow` em handler.** Todo erro que chega ao cliente passa pelo `AppError` (§6).
7. **Nunca troque `sqlx::query!` por `sqlx::query`** para fazer o build passar. Se a macro falha, o cache offline está desatualizado: rode `just prepare`.
8. **Nunca use `localStorage` ou `sessionStorage`** para tokens ou dados do usuário. Refresh token vive no cofre do sistema, via core Rust. Estado de UI vive em memória.
9. **A autorização vem do Discord, e é verificada na entrada *e continuamente*.** O Discord decide quem vê e quem entra numa sala; nós mantemos uma réplica em memória e computamos contra ela. Quando um evento do gateway remove o acesso de alguém, o backend **expulsa o participante da sala do LiveKit em menos de 5 s** — não espera renovação de token. Sessão de mídia dura horas; permissão verificada só na porta vaza por todo esse tempo. Ver [ADR-0010](docs/adr/0010-autorizacao-derivada-do-discord.md).
10. **Nunca faça broadcast de evento.** O conjunto de destinatários é sempre calculado por sala.
11. **Não instale dependência nova sem justificar** em uma linha no PR. Preferir o que já está no `Cargo.toml`/`package.json`.
12. **Não crie abstração antes de três usos reais.** Trait com uma implementação, "service layer" que só repassa e mock de repositório são proibidos: os testes usam Postgres de verdade.

## 3. Layout do workspace

```
.
├── CLAUDE.md
├── justfile
├── .env.example
├── rust-toolchain.toml
├── docs/
│   ├── adr/                    # decisoes arquiteturais. Indice no README.md
│   ├── SRS-v2.0-complemento-screen-share.md
│   ├── ROADMAP.md              # fatias S0-S9
│   ├── websocket.md
│   ├── rest-api.md
│   └── DECISIONS.md            # notas de implementacao por estagio
├── migrations/                 # SQL versionado (sqlx migrate)
├── .sqlx/                      # cache offline do SQLx — VERSIONADO
├── crates/
│   ├── protocol/               # DTOs de wire (REST + WS). Zero logica. Gera TS via ts-rs.
│   ├── domain/                 # regras puras. Sem IO, sem sqlx, sem axum.
│   ├── db/                     # SQLx: pool, repositorios, migrations embarcadas.
│   ├── api/                    # Axum: rotas REST, gateway WS, token LiveKit, admissao.
│   ├── bot/                    # Discord (serenity): pareamento, replica, presenca.
│   └── server/                 # binario unico que compoe api + bot.
├── desktop/
│   ├── src-tauri/              # core Rust: cofre, bandeja, IPC
│   │   ├── capture.rs          # enumeracao de fontes e laco de captura de tela
│   │   ├── publisher.rs        # conexao LiveKit que publica (identidade `~pub`)
│   │   ├── audio.rs            # WASAPI process loopback, excluindo o Discord
│   │   └── share.rs            # comandos Tauri de compartilhamento
│   └── src/                    # React 19 + TS
│       ├── api/                # cliente REST tipado (tipos gerados)
│       ├── gateway/            # cliente WS, resume, dispatch
│       ├── store/              # zustand: estado normalizado
│       ├── media/native.ts     # ponte com o core: fontes, iniciar, parar
│       ├── media/tracks.ts     # so a escolha de camada do espectador
│       ├── features/           # share/, view/, pairing/, settings/
│       └── ui/                 # componentes de base
└── spike/                      # DESCARTAVEL. Nao importar daqui. Ver §9.
```

**Estado real x layout acima.** O disco ainda não está assim. Hoje existem `crates/bridge` e `crates/migrator` (esqueletos de uma linha, saem em S1), não existe `crates/bot`, `desktop/src/` é um scaffold vazio e `spike/` não existe. O layout descreve o alvo; a fatia S1 fecha a diferença.

**Direção de dependência, obrigatória:** `protocol` e `domain` não dependem de ninguém. `db` depende de `domain` e `protocol`. `api` e `bot` dependem de `db`. `server` depende de tudo. Nenhuma seta na direção contrária, nunca.

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

**Rust:** axum 0.8 · tokio · sqlx 0.8 (postgres, macros, uuid, time) · serde · thiserror · tracing + tracing-subscriber (JSON) · jsonwebtoken · uuid (feature `v7`) · sha2 · tower-http (cors, trace, limit) · livekit-api (emissão de token e verificação de webhook) · serenity (Discord).

> Serenity foi escolhido em vez de twilight deliberadamente: tem muito mais exemplos públicos, o que melhora a qualidade do código gerado por agente. Com o pivô ele deixa de ser periférico e vira **componente central** — identidade, autorização e presença passam todas por ele.

**Saem na fatia S1:** `argon2` (não há senha — [ADR-0009](docs/adr/0009-identidade-por-pareamento.md)), `aws-sdk-s3`/`aws-config`/`aws-credential-types` (não há anexos — [ADR-0006](docs/adr/0006-anexos-em-tabela-propria.md)), `validator` (a validação vive em `domain`).

**Frontend:** React 19 · TypeScript strict · Vite · Tailwind · zustand (estado de domínio) · livekit-client (**só para assistir**).

**Core do cliente (`desktop/src-tauri`):** `livekit` 0.9 (SDK Rust, publica) · `windows` 0.61 (WASAPI) · tauri 2 · keyring.

> Este crate está **fora do workspace** e exige `crt-static`: o libwebrtc pré-compilado é ligado ao
> CRT estático, e sem isso o link falha com centenas de `LNK2038`. O `.cargo/config.toml` dele cuida
> disso — e o `justfile` precisa entrar no diretório com `cd`, porque o cargo lê esse arquivo pelo
> diretório **atual** e não pelo `--manifest-path`. O primeiro build baixa ~114 MB e compila C++ por
> vários minutos; reserve alguns GB de disco.

**Saem na fatia S1:** `marked` e `shiki` (não há markdown), `@tanstack/react-virtual` (não há lista longa), `@tanstack/react-query` (sobra REST demais pouco para justificar — o estado chega por WS).

**Proibido no frontend:** qualquer biblioteca de componentes pesada, qualquer state manager além de zustand, e qualquer coisa que rode por frame de vídeo.

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
    #[error("upstream failure")]        Upstream(#[from] UpstreamError),
    #[error("internal")]                Internal(#[from] anyhow::Error),
}
```

Regras:
- `AppError` implementa `IntoResponse` e é o **único** tipo de erro em assinatura de handler: `Result<Json<T>, AppError>`.
- `Internal` nunca vaza detalhe ao cliente. A mensagem real vai para o log com o `request_id`; a resposta traz só o código e o `request_id`.
- Recurso invisível responde **404**, não 403. 403 confirma existência. Vale para canal do Discord que o usuário não pode ver.
- `Upstream` cobre falha do Discord e do LiveKit. Réplica defasada **falha fechada**: recusa, nunca libera.
- Formato do corpo de erro: ver §3 de `docs/rest-api.md`. Não invente outro.

## 7. Convenções de código

**Rust**
- Handlers finos: extraem, validam, chamam `domain`/`db`, mapeiam para DTO. Nenhuma regra de negócio em handler.
- Todo handler tem `#[tracing::instrument(skip(state, ...))]` com o `request_id` no span.
- Queries em `crates/db/src/repo/*.rs`, nunca inline em handler.
- Transações explícitas onde há mais de uma escrita relacionada. Pareamento é transacional por definição.
- **Constantes de permissão do Discord vêm do tipo `Permissions` do serenity.** Nunca escreva o valor do bit à mão.
- Testes de integração usam Postgres real (`testcontainers`). Não escreva mock de repositório. Para o que não se pode chamar em teste — webhook do LiveKit, payload do gateway do Discord — use fixture gravada de um servidor real, não um objeto inventado à mão.

**TypeScript / React**
- `strict: true`. `any` proibido; use `unknown` e refine.
- Tipos de payload **nunca escritos à mão**: vêm de `just types`. Se falta um tipo, adicione no crate `protocol` e regenere.
- Estado de domínio (salas, presença, quem publica) em zustand, **normalizado por ID**. Os eventos do WS escrevem direto no store; o REST só recupera lacuna.
- **Nada no WebView adquire mídia.** Captura, codificação e publicação vivem no core Rust ([ADR-0026](docs/adr/0026-publicacao-no-rust-nativo.md)); `getDisplayMedia` não é chamado em lugar nenhum, e é por isso que não existe seletor nem barra do Chromium. O WebView **assiste**: `desktop/src/media/tracks.ts` guarda só a escolha de camada do espectador ([ADR-0005](docs/adr/0005-modulo-unico-de-midia.md), que mudou de linguagem, não de ideia).
- Nada de trabalho por frame no caminho de render do vídeo: sem observer de resize no elemento de vídeo, sem estado do React atualizando a cada estatística. Estatística de mídia é amostrada em intervalo, nunca no fluxo.
- O elemento de vídeo é montado uma vez por track e nunca remontado por mudança de layout. Remontar derruba o decodificador e custa segundos de tela preta.

## 8. Direção visual (provisória)

Suficiente para não sair um visual de template; revisão de design vem depois.

- Tema escuro, padrão e único. Superfícies em cinzas neutros frios, não em preto puro.
- **O vídeo é o produto.** Toda a interface existe para sair da frente dele: cromo mínimo, controles que somem sozinhos, nenhum painel permanente disputando espaço com a tela compartilhada.
- O aplicativo passa a maior parte do tempo na bandeja, sem janela. O estado "nada acontecendo" é o estado normal e precisa ser barato — ver RNF-03.
- Uma cor de destaque só, para foco e estado ativo. Nada de cor decorativa perto do vídeo: ela desloca a percepção de cor da imagem.
- Tipografia: uma sans para UI, uma mono para números de estatística. Tamanho base 14px.
- Sem sombras difusas, sem gradientes decorativos, sem animação que rode enquanto há vídeo na tela.
- Tokens em `desktop/src/ui/tokens.css`, consumidos via Tailwind. Nenhum valor de cor escrito direto em componente.

## 9. O diretório `spike/`

Contém a prova de conceito de compartilhamento de tela (fatia S0). É **descartável e proibido de importar**. Existe só para registrar números medidos — bitrate real, egress, latência glass-to-glass, CPU de quem compartilha e da VM, e o comportamento com UDP bloqueado — em `spike/RESULTS.md`. Quando S6 começar, o código de produção é escrito do zero.

**Hoje ele não existe, e isso é a maior dívida do projeto.** O SRS da v1 já mandava fazer esse spike antes de tudo; foi ignorado, e onze estágios de plano de controle foram construídos sem nunca verificar se o produto é viável. S0 corrige isso e bloqueia todo o resto.

## 10. Zonas que exigem revisão humana

Não altere sem confirmação explícita:

- `migrations/` — qualquer migration destrutiva (drop, alteração de tipo, remoção de coluna). Inclui a poda proposta em [ADR-0016](docs/adr/0016-poda-por-reescrita-de-migrations.md), que está **Proposto** e não deve ser executada sem aval.
- Configuração de rede e infraestrutura da VM (firewall, portas do LiveKit, TLS, o IP dedicado do TURN).
- Código de captura de mídia e permissões de sistema operacional em `desktop/src-tauri/` — inclui `capture.rs`, `audio.rs` e `publisher.rs` ([ADR-0025](docs/adr/0025-audio-exclui-o-discord.md), [ADR-0026](docs/adr/0026-publicacao-no-rust-nativo.md)).
- Qualquer coisa que toque em credencial, token do bot ou chave de assinatura.
- O modelo de autorização derivado do Discord. Se parecer errado, pergunte; não "corrija".

## 11. Como conduzir uma tarefa

1. Identifique a fatia do roadmap (S0–S9) a que a tarefa pertence. Se não pertence a nenhuma, pergunte antes de escrever código.
2. Leia os ADRs relacionados. Se a tarefa contraria um, o caminho é escrever o ADR que o substitui — não implementar em silêncio.
3. Leia os documentos relevantes da tabela em §1.
4. Escreva o teste que expressa o critério de aceite da fatia **antes** da implementação.
5. Implemente o caminho mais direto que passa no teste. Sem generalizar para casos futuros.
6. Registre as decisões tomadas: ADR se restringe trabalho futuro, `docs/DECISIONS.md` se explica só uma linha.
7. Rode `just check`.
8. Relate: o que mudou, quais arquivos, o que ficou de fora e por quê.

**Não faça refatoração fora do escopo da tarefa.** Se encontrar algo errado em outro lugar, registre em uma linha no relato em vez de consertar.

## 12. Definição de pronto

- `just check` passa inteiro.
- O critério de aceite da fatia tem um teste com nome que o descreve.
- **As decisões tomadas na tarefa estão registradas** — ADR ou `DECISIONS.md`, conforme §11.6.
- Nenhum `TODO` novo sem issue associada.
- Se a tarefa tocou em query: `.sqlx/` regenerado e versionado.
- Se a tarefa tocou em DTO: tipos TypeScript regenerados e versionados.

## 13. Idioma

**DECIDIDO** — inverta aqui, num lugar só, se discordar:

- Identificadores, nomes de arquivo e comentários de código: **inglês**.
- Documentação (`docs/`, este arquivo), textos de interface e conteúdo do produto: **português do Brasil**.
- Mensagens de commit: **inglês**, Conventional Commits (`feat:`, `fix:`, `chore:`).
- Nomes de eventos e campos de wire: **inglês**, `SCREAMING_SNAKE_CASE` para eventos, `snake_case` para campos.
