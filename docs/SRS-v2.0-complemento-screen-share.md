# Especificação Técnica de Requisitos de Software (SRS)

**Projeto:** Complemento de Compartilhamento de Tela para Discord (Desktop)
**Versão:** 2.0.0
**Status:** Aprovado para desenvolvimento
**Data:** 12 de setembro de 2026
**Substitui:** `SRS-v1.1-plataforma-comunicacao.md` (v1.2.0, 29/08/2026), integralmente

---

## 0. Relação com a v1.2

A v1.2 especificava uma plataforma privada de comunicação que substituiria um servidor do
Discord: chat, DMs, busca, voz, vídeo, tela, migração de histórico e ponte bidirecional
permanente. A v2.0 **não é uma revisão dela** — é outro produto, com outra proposta de
valor, descrito em [ADR-0008](adr/0008-complemento-ao-discord.md).

Numeração de requisitos recomeça do zero. Os identificadores RF/RNF da v1.2 estão mortos;
citações a "RF-22a" ou "RNF-04" em commits e documentos antigos referem-se ao documento
antigo e não devem ser reaproveitadas.

O que sobrevive da v1.2, e por quê:

| Da v1.2 | Estado na v2.0 |
|---|---|
| Autenticação por JWT curto + refresh opaco rotativo com detecção de reúso | Mantida integralmente. Só muda o que acontece **antes** dela ([ADR-0009](adr/0009-identidade-por-pareamento.md)) |
| Gateway WebSocket com `HELLO`/`IDENTIFY`/`RESUME` e buffer de retomada | Mantido; o conjunto de eventos encolhe de 30 para ~6 |
| Emissão de token do LiveKit, guard de admissão, verificação de webhook | Mantidos e adaptados ao snowflake do Discord |
| SFU e Postgres auto-hospedados, UUIDv7 na aplicação, keyset | Mantidos ([ADR-0002](adr/0002-sfu-auto-hospedado.md), [ADR-0004](adr/0004-uuidv7-na-aplicacao.md), [ADR-0015](adr/0015-postgres-com-schema-reduzido.md)) |
| Mensagens, anexos, busca, DMs, cargos, convites, ponte, migração | Removidos ([ADR-0016](adr/0016-poda-por-reescrita-de-migrations.md)) |

---

## 1. Visão Geral

### 1.1 Objetivo

Entregar compartilhamento de tela em alta qualidade para comunidades que continuam
vivendo no Discord, em regiões onde o compartilhamento de tela do Discord é bloqueado por
lei ou degradado a ponto de inutilidade.

O produto **não substitui o Discord**. Texto, voz, comunidade, identidade e permissões
permanecem lá. Nós somos o plano de mídia da tela, e nada além disso.

### 1.2 Princípio de escopo

> Se o Discord já faz, não reimplemente — integre.

Toda proposta de recurso passa por essa frase antes de qualquer discussão técnica. Ela é o
que impede o produto de virar, por acidente e uma feature de cada vez, o clone que o
[ADR-0008](adr/0008-complemento-ao-discord.md) descartou.

### 1.3 Perfil de uso dimensionante

Premissa de projeto, não estimativa. Todo dimensionamento em §7 deriva daqui.

| Dimensão | Valor |
|---|---|
| Comunidade por instância | 10 a 30 pessoas |
| Uso central | Uma pessoa compartilha gameplay ou estudo em 1080p60; 4 a 10 assistem; sessões longas |
| Publicadores simultâneos por sala | N, com teto configurável (`ROOM_MAX_PUBLISHERS`, padrão 10 desde 2026-09-17; era 2). Cada um multiplica o egress — ver RF-32 |
| Espectadores simultâneos por sala | Até 10 |
| Plataforma dos clientes | Windows 10/11 x86_64 (100%) |
| Rede alvo | Comum. Não há bloqueio regional a mídia em tempo real ([ADR-0020](adr/0020-o-bloqueio-e-do-discord-nao-da-rede.md)); o caso difícil é CGNAT, como em qualquer produto de WebRTC |
| Hospedagem | Uma instância por comunidade, auto-hospedada |

> A linha "rede alvo" dizia "hostil a mídia em tempo real" e era a origem do RNF-01.
> Corrigida em 2026-09-14: quem desligou o compartilhamento de tela foi o próprio
> Discord, não a rede ([ADR-0020](adr/0020-o-bloqueio-e-do-discord-nao-da-rede.md)).

### 1.4 Escopo do produto

- **Cliente desktop:** shell Tauri v2 (WebView2), UI em React 19 + TypeScript + Tailwind.
- **Backend:** Rust (Axum + Tokio) — emissão de token, admissão, estado de sala.
- **Bot do Discord:** identidade, réplica de autorização, presença e anúncio.
- **Mídia:** LiveKit SFU auto-hospedado, com TURN para atravessar CGNAT.
- **Banco:** PostgreSQL, com cerca de cinco tabelas.

### 1.5 Fora de escopo, definitivamente

Mensagens de texto, anexos, busca, conversas diretas, reações, menções, cargos próprios,
convites, migração de histórico, ponte de mensagens, microfone, câmera, gravação de
sessão, controle remoto, anotação sobre a tela, clientes móveis ou web, e link de
compartilhamento sem Discord.

Cada item tem rejeição registrada em `docs/adr/`. Reabrir qualquer um exige ADR novo que
substitua o anterior.

---

## 2. Arquitetura

```
+-----------------------------------------------------------------------+
|                     TAURI V2 DESKTOP (Windows)                        |
|  +-----------------------------+   +-------------------------------+  |
|  | WebView2 (React 19 / TS)    |   | Core Nativo (Rust)            |  |
|  | - LiveKit JS SDK            |   | - Cofre do Windows (keyring)  |  |
|  | - Captura de tela (video)   |<--| - Captura WASAPI por processo |  |
|  | - AudioWorklet -> track     |IPC|   (PCM, ~384 KB/s)  [S9]      |  |
|  | - Tipos gerados via ts-rs   |   | - Bandeja, notificacao        |  |
|  +-----------------------------+   +-------------------------------+  |
+------------+--------------------------------+-------------------------+
             |                                |
      [HTTPS / WSS :443]            [SRTP - UDP direto]
             |                      [TURN/TLS - fallback CGNAT]
             |                                |
+------------v--------------------------------v-------------------------+
|                  VM (backend + SFU + TURN + DB)                       |
|  +---------------------+   +--------------------------------------+   |
|  |  Backend Axum       |   |  LiveKit SFU                         |   |
|  |  - REST (poucas)    |   |  - UDP 50000-60000                   |   |
|  |  - WS Gateway       |<--+  - TURN/TLS (fallback CGNAT)         |   |
|  |  - Token LiveKit    |   |  - Webhooks -> Axum                  |   |
|  |  - Admissao/egress  |   +--------------------------------------+   |
|  +----------+----------+                                              |
|             |              +--------------------------------------+   |
|  +----------v----------+   |  Bot Discord (serenity)              |   |
|  |  PostgreSQL         |   |  - Replica de guild/canal/cargo      |   |
|  |  ~5 tabelas         |   |  - Estados de voz                    |   |
|  +---------------------+   |  - Pareamento e anuncio              |   |
|                            +-------------------+------------------+   |
+------------------------------------------------|---------------------+
                                                  v
                                        [ Discord Gateway / API ]
```

A réplica do Discord vive **em memória**, reconstruída no `GUILD_CREATE` a cada conexão do
gateway. Não há tabela para ela: o Discord é a fonte de verdade, e persistir uma cópia só
criaria uma segunda que diverge.

Decisões arquiteturais em `docs/adr/`. Este documento descreve o sistema; os ADRs
descrevem por que ele não é de outro jeito.

---

## 3. Requisitos Funcionais

### 3.1 Módulo 1 — Identidade

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-01 | Pareamento por código | Comando de barra no Discord emite código de uso único, 5 min de validade, resposta efêmera. O aplicativo troca o código pelo par de tokens. O código nunca é gravado em claro: persiste-se o hash SHA-256. | Must |
| RF-02 | Sessão e renovação | Access token JWT de 15 min; refresh token opaco rotativo de 30 dias, em família, com revogação da família inteira ao detectar reúso. | Must |
| RF-03 | Cofre do sistema | O refresh token é persistido pelo core Rust no cofre de credenciais do Windows. Nunca em `localStorage`, `sessionStorage` ou arquivo. | Must |
| RF-04 | Perfil espelhado | Nome de exibição e avatar vêm do Discord, atualizados no pareamento e por evento do gateway. Não há edição de perfil no produto. | Must |
| RF-05 | Encerrar sessão | Logout revoga a família de refresh e limpa o cofre. Desparear exige novo código. | Should |

### 3.2 Módulo 2 — Autorização

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-06 | Réplica do estado do Discord | O bot mantém em memória guilds, canais de voz, cargos e membros, a partir do `GUILD_CREATE` e de eventos incrementais. Intents: `GUILD_MEMBERS` e estados de voz. `MESSAGE_CONTENT` **não** é usado. | Must |
| RF-07 | Admissão à sala | Entrar numa sala exige `VIEW_CHANNEL` e `CONNECT` no canal de voz correspondente, computados contra a réplica com as constantes do serenity — nunca com bits escritos à mão. | Must |
| RF-08 | Revogação ao vivo | Perder o acesso no Discord (saída do servidor, perda de cargo, canal restrito, banimento) **desconecta o usuário da sala do LiveKit em menos de 5 s**, sem esperar renovação de token. | Must |
| RF-09 | Modo degradado | Réplica defasada além do limiar configurado recusa entradas novas e mantém as sessões em curso. Falha fechada, nunca aberta. | Must |

### 3.3 Módulo 3 — Sala

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-10 | Sala derivada do canal de voz | O nome da sala do LiveKit é derivado do snowflake do canal de voz do Discord. Não existe seletor de sala na interface. | Must |
| RF-11 | Entrada e saída automáticas | Entrar num canal de voz do Discord faz o aplicativo entrar na sala correspondente sem interação; sair faz o inverso. | Must |
| RF-12 | Estado da sala | Quem está presente e quem está publicando, propagado por WebSocket a partir dos webhooks do LiveKit e dos estados de voz do Discord. | Must |

### 3.4 Módulo 4 — Compartilhamento

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-13 **[M]** | Tela inteira com áudio | Captura de tela inteira com áudio. Resolução e taxa de quadros são escolhidas por **quem publica** (RF-36), porque é ele quem paga o encode. `contentHint: 'motion'`, `degradationPreference: 'maintain-framerate'`; duas camadas de simulcast derivadas da escolha; VP9 preferencial, H.264 como alternativa. | Must |
| RF-14 | Janela específica | Captura de janela individual, pelo `DesktopCapturer` do core Rust. O áudio não é da janela e sim do sistema menos o Discord (RF-29), e a interface diz isso no momento da escolha, sem eufemismo. | Must |
| RF-15 | Teto de publicadores | Máximo configurável de publicadores simultâneos por sala, padrão 2, por controle de admissão no momento de emitir o token. | Must |
| RF-16 | Adaptação de qualidade | `adaptiveStream` e `dynacast` obrigatoriamente habilitados. Espectador que não está vendo não recebe camada alguma. | Must |
| RF-17 | Encerramento | Parar de compartilhar despublica as tracks e propaga o estado. Fechar o aplicativo ou perder a conexão produz o mesmo efeito, por `departure_timeout` do SFU. | Must |

### 3.5 Módulo 5 — Visualização

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-18 | Assistir | Assinar a track de quem publica, com primeiro frame dentro da meta do RNF-02. | Must |
| RF-19 **[M]** | Seletor de qualidade do espectador | Automático, alta ou baixa, **entre as camadas que o publicador está enviando** — nunca uma lista que inclua combinação inexistente ([ADR-0023](adr/0023-quem-publica-escolhe-resolucao-e-fps.md)). `adaptiveStream` pode descer abaixo da escolha quando a janela é pequena ou está oculta; economizar banda de quem não olha vence a preferência declarada. Sem paywall: 1080p60 é o padrão, não recurso pago. | Must |
| RF-20 | Tela cheia | Modo tela cheia com controles que somem sozinhos. | Should |
| RF-21 | Estatísticas do publicador | Bitrate, fps, resolução efetiva, número de espectadores e CPU de encode, visíveis para quem compartilha. | Should |
| RF-22 | Lista de espectadores | Quem está assistindo, visível para quem compartilha. | Should |

### 3.6 Módulo 6 — Presença dentro do Discord

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-23 | Anúncio por mensagem editada | Ao iniciar uma sessão, o bot publica **uma** mensagem no canal de texto associado e a **edita** ao longo da sessão (contagem de espectadores, encerramento). Uma sessão inteira produz uma mensagem, nunca várias. | Must |
| RF-24 | Link profundo | O anúncio traz um link que abre o aplicativo direto na sala; sem o aplicativo instalado, leva à página de download. | Must |
| RF-25 | Respeito ao limite de taxa | O anúncio respeita o limite de edição por canal, com coalescência de atualizações. Renomear canal de voz é proibido como mecanismo de presença: o limite é de duas renomeações por 10 min e o recurso quebraria sob uso normal. | Must |

### 3.7 Módulo 7 — Integração desktop

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-26 | Bandeja do sistema | O aplicativo roda em segundo plano, minimiza para a bandeja e restaura pelo menu de contexto. É o modo de operação normal: ele fica aberto o dia todo. | Must |
| RF-27 | Notificação nativa | Notificação ao início de um compartilhamento na sala em que o usuário está. Suprimida se ele já estiver assistindo ou com a janela em foco. | Should |
| RF-28 | Atualização automática | Instaladores `.msi` por GitHub Releases, com verificação de assinatura. | Should |

### 3.8 Módulo 8 — Áudio por aplicativo

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-29 **[M]** | Áudio do sistema sem o Discord | No Windows, captura do áudio do sistema **excluindo a árvore de processos do Discord**, via WASAPI process loopback em modo `EXCLUDE_TARGET_PROCESS_TREE`, no core Rust, entregue direto ao `NativeAudioSource` do publicador — **sem IPC e sem `AudioContext`**, porque captura e codificação passaram a viver no mesmo processo ([ADR-0026](adr/0026-publicacao-no-rust-nativo.md)). O usuário não escolhe processo: sai tudo menos o Discord. Ver [ADR-0025](adr/0025-audio-exclui-o-discord.md). | Must |
| RF-30 | Fallback declarado | Onde o process loopback não estiver disponível, cai para áudio do sistema **com aviso explícito** de que a voz dos outros participantes será retransmitida. Compartilhar sem áudio é sempre uma opção de um clique. | Must |


### 3.9 Módulo 9 — Várias telas ao mesmo tempo

Tudo aqui nasce da fatia S7. O produto deixa de ser "uma tela por sala" e passa a
ser N publicadores para N espectadores.

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-31 **[N]** | Várias telas simultâneas | Uma sala comporta até `ROOM_MAX_PUBLISHERS` telas ao mesmo tempo, e o espectador vê todas. O cliente mantém uma assinatura por publicador, não uma só. | Must |
| RF-32 **[N]** | Grade e foco | Layout em grade com todas as telas, e foco em uma. **A grade assina a camada baixa; só o foco pede a alta.** Não é refinamento: com N telas visíveis o egress multiplica por N, e é o layout que decide o custo ([ADR-0023](adr/0023-quem-publica-escolhe-resolucao-e-fps.md)). | Must |
| RF-33 **[N]** | Destacar em outra janela | Uma tela pode ser destacada para uma janela do sistema, arrastável para outro monitor, via Document Picture-in-Picture. O elemento de vídeo é **movido**, nunca remontado — remontar derruba o decodificador. Limite de uma janela destacada por vez ([ADR-0022](adr/0022-destacar-tela-usa-document-pip.md)). | Must |
| RF-34 **[N]** | Dono e tempo de transmissão | Cada tela exibe de quem é e há quanto tempo está no ar. O início vem do servidor (`share_sessions.started_at`), não do momento em que o espectador entrou: quem chega depois precisa ver o tempo real da transmissão. | Must |
| RF-35 **[N]** | Volume por tela | Cada tela tem controle de volume independente, do silêncio ao máximo, e o estado sobrevive à troca de foco. | Must |
| RF-36 **[N]** | Resolução e fps no publicador | Quem compartilha escolhe entre 1080p60, 1080p30, 720p60 e 720p30. A escolha define a camada alta; a baixa é derivada. Trocar durante a transmissão republica a track, e a interface diz isso em vez de parecer travada. | Must |
| RF-37 **[N]** | Interface própria de compartilhamento | Todo o fluxo é nosso, **seletor de fonte incluído**: telas e janelas são enumeradas pelo `DesktopCapturer` do core Rust e apresentadas na nossa interface. Como `getDisplayMedia` deixa de ser chamado, a barra "você está compartilhando" do Chromium não aparece. Ver [ADR-0026](adr/0026-publicacao-no-rust-nativo.md). | Should |

### 3.10 Módulo 10 — Marcação de quem transmite

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-38 **[N]** | Tag `[LIVE]` no apelido | Quem está transmitindo recebe o prefixo `[LIVE] ` no apelido do servidor, e o perde ao parar. Exige `MANAGE_NICKNAMES`. | Should |
| RF-39 **[N]** | Guardas da marcação | Dono do servidor e membros com cargo acima do bot são pulados em silêncio — é limitação do Discord, sem contorno para o dono. O apelido anterior é salvo e restaurado **exatamente**, inclusive o caso de não haver apelido. Marcar e desmarcar são idempotentes. Ver [ADR-0024](adr/0024-tag-live-no-apelido.md). | Must |
| RF-40 **[N]** | Limpeza no arranque | O servidor desmarca, ao subir, quem ficou marcado por uma queda, antes de aceitar sessão nova. Sem isto, um apelido alheio fica sujo até alguém notar. | Must |

---

## 4. Requisitos Não Funcionais

### 4.1 Transporte — o requisito que define o produto

**RNF-01 (Resiliência de transporte) [M].** Uma sessão de compartilhamento 1080p
estabelece e se sustenta por 20 minutos **com todo o UDP de saída bloqueado no cliente**,
via TURN. Verificável com uma regra de firewall local.

> Era o requisito que definia o produto, sob a premissa de um adversário de rede que não
> existe. Rebaixado em 2026-09-14 ([ADR-0020](adr/0020-o-bloqueio-e-do-discord-nao-da-rede.md)):
> continua sendo critério de aceite de S2, porque quem está atrás de CGNAT só conecta pelo
> relay, mas não é mais bloqueio de release.

### 4.2 Latência e custo

**RNF-02 (Latência de mídia).** Glass-to-glass p95 < 300 ms, medido **no caminho
relayado**, não no melhor caso — na região alvo o relay é o caso comum. Tempo do clique em
"assistir" até o primeiro frame renderizado: p95 < 3 s.

**RNF-03 (Custo em repouso).** O aplicativo fica aberto o dia inteiro. Em repouso, na
bandeja, sem sala ativa: < 1% de CPU e < 150 MB de RSS agregado. Assistindo: < 500 MB.
Publicando 1080p60: < 700 MB. Medição somando a árvore de processos após 30 min.

**RNF-04 (Custo de encode).** CPU do compartilhador durante 1080p60 registrada em S0 e
tratada como orçamento: uma regressão que a estoure é defeito, não característica.
Aceleração de hardware é preferida onde o WebView2 a expuser.

**RNF-05 (Orçamento de egress).** Teto de 6 TB/mês, 60% da franquia de 10 TB, com alerta
em 70% do teto. Referência: uma tela 1080p60 a ~6 Mbps para 8 espectadores gera ~48 Mbps,
ou ~21,6 GB/h — cerca de **277 h/mês** antes de esgotar o teto. Esta é a única variável de
custo do produto e precisa ser **medida** pelo backend, não estimada. `adaptiveStream` e
`dynacast` são obrigatórios; o teto de publicadores é o guard de última instância.

> O relay por TURN **não** dobra o egress: TURN e SFU rodam na mesma VM, então o repasse é
> local. O custo do relay é CPU, não banda de saída.

### 4.3 Segurança e privacidade

**RNF-06 (Tokens de mídia).** JWT do LiveKit com escopo de uma sala, TTL ≤ 3600 s imposto
em código e não só em configuração, renovado silenciosamente. Token de espectador não
recebe permissão de publicação alguma.

**RNF-07 (Segredos).** Token do bot, segredo do LiveKit e chave de assinatura JWT vivem em
variáveis de ambiente ou arquivo com permissão 600. Nunca no repositório, nunca em tabela
sem criptografia.

**RNF-08 (Privacidade).** O produto registra **quem publicou, quando e por quanto tempo**,
e a contagem de pico de espectadores — insumos do orçamento de egress. Não registra quem
assistiu o quê. Nenhum conteúdo de tela ou áudio é gravado ou persistido em lugar algum.

**RNF-09 (Persistência e backup).** `pg_dump` comprimido diário para fora do provedor,
retenção de 14 dias, com teste de restauração mensal registrado. Alvo de crescimento do
banco: < 1 GB.

### 4.4 Operação

**RNF-10 (Plataforma).** Windows 10/11 x86_64 é a única plataforma suportada, testada e
distribuída. O código não deve conter dependências que impeçam compilar para macOS e
Linux, sabendo que a captura com áudio não existe nos WebViews dessas plataformas.

**RNF-11 (Reconexão).** Backoff exponencial com jitter no gateway WebSocket e no WebRTC.
Reconectar não deve exigir nova interação: se o usuário continua no canal de voz do
Discord, ele volta à sala sozinho.

**RNF-12 (Portabilidade de infraestrutura).** VM inteira descrita como código (cloud-init,
`docker compose`, configuração do Caddy e do TURN), versionada. Objetivo: migrar para um
VPS pago equivalente em ≤ 2 h, sem alterar código de aplicação.

**RNF-13 (Observabilidade).** Métricas mínimas: egress acumulado no mês, sessões ativas,
espectadores por sala, proporção de conexões relayadas contra diretas, idade da réplica do
Discord, latência de emissão de token. Logs estruturados em JSON, retenção de 7 dias.

> A proporção de conexões relayadas é a métrica mais importante do produto: é ela que diz
> se o RNF-01 está sendo exercitado de verdade em produção ou só em teste.

**RNF-14 (Implantação).** Instância única, sem fan-out entre processos. Uma implantação
derruba as conexões WebSocket ativas; mitigado pelo RNF-11, com janela fora do pico.

---

## 5. Modelo de Dados

Convenções inalteradas: PKs UUIDv7 geradas na aplicação, sem `DEFAULT` no banco;
snowflakes do Discord como `BIGINT`; paginação sempre por keyset.

```sql
-- ============ IDENTIDADE ============

-- Cache do perfil do Discord. Nao ha senha, e-mail nem edicao de perfil.
CREATE TABLE users (
    id               UUID PRIMARY KEY,
    discord_user_id  BIGINT UNIQUE NOT NULL,
    username         VARCHAR(32)  NOT NULL,
    display_name     VARCHAR(64),
    avatar_url       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ
);

-- Codigo de pareamento: credencial de curta duracao, uso unico.
-- Guarda-se o hash, nunca o codigo.
CREATE TABLE pairing_codes (
    id                UUID PRIMARY KEY,
    code_hash         CHAR(64) UNIQUE NOT NULL,
    discord_user_id   BIGINT      NOT NULL,
    discord_guild_id  BIGINT      NOT NULL,
    expires_at        TIMESTAMPTZ NOT NULL,
    consumed_at       TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_pairing_pending ON pairing_codes (expires_at) WHERE consumed_at IS NULL;

-- Inalterada em relacao a v1.2: reuso de token consumido revoga a familia inteira.
CREATE TABLE refresh_tokens (
    id          UUID PRIMARY KEY,
    family_id   UUID NOT NULL,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  CHAR(64) UNIQUE NOT NULL,
    user_agent  TEXT,
    issued_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at  TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ
);
CREATE INDEX idx_refresh_family ON refresh_tokens (family_id);
CREATE INDEX idx_refresh_user   ON refresh_tokens (user_id) WHERE revoked_at IS NULL;

-- ============ SESSOES DE COMPARTILHAMENTO ============

-- Insumo do orcamento de egress e do historico do publicador.
-- peak_viewers e uma CONTAGEM: nao se registra quem assistiu (RNF-08).
CREATE TABLE share_sessions (
    id                  UUID PRIMARY KEY,
    discord_channel_id  BIGINT NOT NULL,
    publisher_id        UUID   NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    started_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at            TIMESTAMPTZ,
    peak_viewers        INT    NOT NULL DEFAULT 0,
    egress_bytes        BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX idx_sessions_open  ON share_sessions (discord_channel_id) WHERE ended_at IS NULL;
CREATE INDEX idx_sessions_month ON share_sessions (started_at);

-- ============ PRESENCA NA SALA ============
-- UNLOGGED: estado efemero. Sem socket nao ha presenca; nao precisa sobreviver a crash.
CREATE UNLOGGED TABLE room_presence (
    user_id             UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    discord_channel_id  BIGINT  NOT NULL,
    session_id          TEXT    NOT NULL,
    publishing          BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_presence_channel ON room_presence (discord_channel_id);
```

Cinco tabelas. **Não há tabela de guild, canal, cargo ou membro**: esse estado é réplica do
Discord e vive em memória ([ADR-0010](adr/0010-autorizacao-derivada-do-discord.md)).

---

## 6. Fluxos Críticos

### 6.1 Pareamento

```
Usuario roda /tela parear no Discord
        |
   Bot gera codigo (uso unico, 5 min), responde EFEMERO
   grava apenas o hash em pairing_codes
        |
   Usuario digita o codigo no aplicativo
        |
   POST /pair {code}
        |
   backend resolve por hash, em tempo constante
        |-- expirado / consumido / inexistente -> mesma resposta, mesmo tempo
        |-- valido -> consumed_at = now()
        |
   upsert em users a partir do perfil do Discord
        |
   emite access token + refresh token
        |
   core Rust grava o refresh no cofre do Windows
```

### 6.2 Admissão e revogação ao vivo

```
Usuario entra no canal de voz do Discord
        |
   bot recebe estado de voz -> notifica backend
        |
   backend computa VIEW_CHANNEL + CONNECT contra a replica
        |-- sem permissao -> nada acontece (o app nao entra na sala)
        |-- replica defasada alem do limiar -> recusa (falha fechada)
        |-- ok -> emite token do LiveKit com escopo daquela sala
        |
   app entra na sala, assina quem estiver publicando

--- em paralelo, pelo tempo que a sessao durar ---

evento do gateway do Discord altera acesso
   (saiu do servidor | perdeu cargo | overwrite mudou | banido)
        |
   backend recomputa os presentes daquela sala
        |
   quem perdeu acesso -> RemoveParticipant no LiveKit, em < 5 s
```

Verificar permissão só na porta é permissão que vaza pelo tempo inteiro da sessão — e
sessões aqui duram horas. A revogação contínua não é refinamento, é parte do requisito.

### 6.3 Início de um compartilhamento

```
Publicador escolhe tela ou janela
        |
   controle de admissao: ha vaga no teto de publicadores?
        |-- nao -> 409, com o motivo nomeado
        |-- sim -> token com permissao de publicar screen_share[_audio]
        |
   captura com contentHint 'motion', camadas de simulcast
        |
   LiveKit -> webhook track_published -> backend
        |
   room_presence.publishing = true ; abre share_sessions
        |
   fan-out por WS para a sala  +  bot publica UMA mensagem no Discord
        |
   ao longo da sessao: a MESMA mensagem e editada (espectadores, fim)
```

---

## 7. Infraestrutura e Custo

| Componente | Solução | Limite gratuito | Estratégia |
|---|---|---|---|
| Backend, SFU, TURN, DB, proxy | VM ARM 2 OCPU / 12 GB | 200 GB de bloco; ~1 Gbps por OCPU; 10 TB de egress/mês | Egress é o único recurso escasso (RNF-05) |
| Banco | PostgreSQL 16 auto-hospedado | 200 GB (uso previsto < 1 GB) | Dump diário para fora do provedor |
| Mídia | LiveKit OSS auto-hospedado | Limitado por egress | `adaptiveStream` + `dynacast` + teto de publicadores |
| TURN | LiveKit TURN/TLS em 443 | — | **IP público dedicado**, para não disputar 443 com o Caddy |
| Distribuição | GitHub Releases | Ilimitado | `.msi` assinado, atualização automática |
| Objetos | Cloudflare R2 | 10 GB | **Apenas** destino do `pg_dump`, por ferramenta de sistema. A aplicação não fala com o R2 |

### 7.1 Armadilhas operacionais conhecidas

1. **Duas portas 443.** Caddy e TURN não podem dividir a mesma. Exige IP secundário na VM,
   hostname e certificado próprios. É a parte mais fácil de errar na implantação.
2. **iptables da imagem base.** Liberar portas no painel do provedor não basta: as imagens
   trazem regras locais que bloqueiam tudo além da 22. Ajustar no cloud-init.
3. **CPU de relay.** Todo cliente relayado consome CPU de TURN na VM. Precisa entrar no
   orçamento, e é medido em S0.
4. **Intents privilegiados.** `GUILD_MEMBERS` exige habilitação no portal do Discord.
   Abaixo de 100 servidores dispensa aprovação, mas não dispensa o clique — e sem ele a
   réplica nasce incompleta e a autorização falha fechada para todo mundo.

---

## 8. Matriz de Riscos

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| **A rede bloqueia nossa mídia como bloqueia a do Discord** | Média | **Crítico — o produto perde a razão de existir** | RNF-01 e [ADR-0013](adr/0013-turn-tls-443-primario.md); medido em S0, antes de qualquer investimento em UI |
| 1080p60 não se sustenta pelo caminho relayado | Média | Alto | Medido em S0; a resposta é baixar o alvo com número na mão, e registrar em ADR |
| Bot do Discord fora do ar ou com intent revogado | Média | Alto | RF-09: modo degradado, falha fechada, sessões em curso preservadas |
| Eco de áudio: a voz do Discord volta pela tela compartilhada | **Alta** | Alto | RF-29/RF-30; até S9, compartilhar sem áudio ou aceitar conscientemente |
| Eco de áudio: o som da tela alheia volta pela nossa captura | Média | Médio | A exclusão do WASAPI aceita um processo só, gasto no Discord. Mitigado silenciando as telas alheias enquanto se transmite áudio ([ADR-0028](adr/0028-silenciar-telas-alheias-ao-transmitir-audio.md)) |
| `webrtc-sys` quebra o build do cliente numa atualização de toolchain | Média | Alto | Versão fixada junto com o par do [ADR-0019](adr/0019-versoes-do-livekit-sao-um-par.md); `LK_CUSTOM_WEBRTC` permite apontar para um libwebrtc próprio se necessário |
| Egress estoura o teto | Média | Alto | RNF-05 medido pelo backend, alerta em 70%, teto de publicadores |
| CPU da VM satura entre SFU, TURN e encode de relay | Média | Médio | Medido em S0; limites por `docker compose`; RNF-13 para detectar |
| Instância recuperada por ociosidade do provedor | Média | Alto | Conta convertida para Pay As You Go |
| Provedor reduz a cota gratuita de novo | Média | Alto | RNF-12: infraestrutura como código, migração em ≤ 2 h |
| Perda da VM (sem alta disponibilidade) | Baixa | Médio | Dados são poucos e reconstruíveis; o que dói é o downtime, não a perda |
| Legalidade de operar na jurisdição alvo | — | — | Decisão do operador que hospeda, não deste projeto. A arquitetura responde não centralizando e não registrando quem assistiu o quê (RNF-08) |

---

## 9. Roadmap

Em `docs/ROADMAP.md`, fatias S0 a S9. O roadmap F0–F8 da v1.2 está aposentado.

**S0 vem antes de tudo.** O SRS v1.2 já dizia isso do spike de screen share, foi ignorado,
e o projeto construiu onze estágios de plano de controle sem nunca verificar se o produto
é viável. Não repetir.

---

## 10. Pontos a confirmar durante a implementação

Nenhum bloqueia o início; todos têm padrão definido e são ajustáveis sem retrabalho
estrutural. Quando um for resolvido, vira ADR.

| ID | Ponto | Padrão adotado | Revisar quando |
|---|---|---|---|
| P-01 | Teto de publicadores simultâneos por sala | **10** (era 2, revisado em 2026-09-17) | Se o egress medido (RNF-05) se aproximar do teto do plano hospedado |
| P-02 | Limiar de defasagem da réplica que recusa entradas | 60 s | Após medir a frequência real de queda do gateway |
| P-03 | Validade do código de pareamento | 5 min | Se gerar atrito recorrente |
| P-04 | Alvo de captura padrão | 1080p60 | Depende inteiramente do resultado de S0 |
| P-05 | Canal de texto que recebe o anúncio | O canal associado ao canal de voz do Discord | Se comunidades quiserem um canal dedicado |
| P-06 | Link de compartilhamento sem Discord | Não existe | Só com ADR novo substituindo o [ADR-0011](adr/0011-sala-e-o-canal-de-voz.md) |

---

## 11. Convenções de Desenvolvimento em Harness de IA

Inalteradas em relação à v1.2 §11, com dois acréscimos:

1. `SQLX_OFFLINE=true` com `.sqlx/` versionado.
2. Comando único de verificação: `just check`.
3. Contrato primeiro: tipos em Rust são a fonte da verdade; `ts-rs` gera o TypeScript.
4. `CLAUDE.md` na raiz, com as regras não negociáveis.
5. Zonas proibidas ao agente sem revisão humana: migrations destrutivas, código de captura
   de mídia e permissões de SO, credenciais e configuração de rede da VM.
6. Testes de integração com Postgres real, não mocks de repositório.
7. **[N]** Toda decisão que restrinja trabalho futuro vira ADR em `docs/adr/` antes de a
   tarefa ser dada como pronta ([ADR-0007](adr/0007-governanca-de-decisoes.md)).
8. **[N]** Fixtures determinísticos para o que não se pode chamar em teste: os corpos de
   webhook do LiveKit já são fixtures gravadas byte a byte de um servidor real; o mesmo
   vale para os payloads do gateway do Discord.
