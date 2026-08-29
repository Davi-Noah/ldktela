# Especificação Técnica de Requisitos de Software (SRS)

**Projeto:** Plataforma Privada de Comunicação em Tempo Real (Desktop)
**Versão:** 1.2.0
**Status:** Aprovado para desenvolvimento. Escopo fechado — as 4 decisões pendentes na v1.1.0 foram resolvidas (§10)
**Data:** 29 de agosto de 2026
**Substitui:** v1.1.0 (29/08/2026), v1.0.0-PROD (28/08/2026)

---

## 0. Changelog

Esta revisão existe porque a v1.0.0 continha três premissas factualmente incorretas e um requisito tecnicamente inalcançável. As mudanças abaixo não são refinamentos: elas alteram a topologia de infraestrutura e o escopo de plataformas.

### v1.0.0 → v1.1.0

| # | Mudança | Motivo |
|---|---|---|
| C-01 | **LiveKit Cloud → LiveKit self-hosted** na VM Oracle | A cota gratuita da LiveKit Cloud é de 5.000 minutos de WebRTC/mês, não 50.000. Com screen share como uso central, isso equivale a poucas horas por mês. Além disso, no plano gratuito a cota é teto rígido: ao estourar, novas conexões falham. |
| C-02 | **Postgres gerenciado (500 MB) → Postgres self-hosted** na mesma VM | O volume Always Free de 200 GB elimina o teto de ~1,2M mensagens e destrava índices de busca full-text, que não caberiam em 500 MB. |
| C-03 | **RNF-11 (multiplataforma) rebaixado**: v1 suporta apenas Windows 10/11 x86_64 | Tauri usa o WebView do sistema. WebView2 (Chromium) entrega captura de tela com áudio do sistema; WKWebView e WebKitGTK não. Suportar as três plataformas exigiria SDK Rust nativo do LiveKit + pipeline de renderização próprio — semanas de custo para uma base de usuários 100% Windows. |
| C-04 | **RF-16 dividido em RF-16a e RF-16b** | No Chromium, compartilhamento de *janela específica* não transporta áudio. Áudio do sistema só existe em captura de tela inteira. O requisito original era inalcançável como escrito em qualquer plataforma. |
| C-05 | **RNF-10 reescrito**: cota de minutos → orçamento de egress mensal | Consequência direta de C-01. |
| C-06 | **RNF-01, RNF-03 e RNF-04 reescritos como metas mensuráveis** | Os valores originais (<50 ms fim-a-fim, 120 MB de RAM, 60 fps fixos) não eram atingíveis nem verificáveis, e servem de insumo direto para agentes de codificação. |
| C-07 | **Modelo de dados expandido de 5 para 17 tabelas** | O schema v1.0.0 não sustentava RF-02, RF-07, RF-12 nem RF-26. Faltavam membros, cargos, overwrites de canal, convites, sessões, estado de leitura e estado de voz. |
| C-08 | **UUIDv4 → UUIDv7** em todas as chaves primárias | v4 é aleatório: fragmenta o B-tree na inserção, infla índices e impede paginação por keyset estável. |
| C-09 | Novos RNF-13 (portabilidade de infraestrutura) e RNF-14 (observabilidade) | O provedor de infraestrutura reduziu a cota Always Free de Ampere A1 sem aviso público em junho de 2026. Dependência de fornecedor único vira risco de primeira ordem. |
| C-10 | Nova §11: convenções de desenvolvimento em harness de IA | Todo o código de aplicação será escrito em Claude Code. Isso impõe requisitos de tooling que, se ignorados, degradam a qualidade da geração. |

### v1.1.0 → v1.2.0

Fechamento das quatro decisões de escopo que estavam em aberto. Nenhuma delas foi rebaixada: as quatro entraram, e o custo está distribuído no schema, nos requisitos e no roadmap.

| # | Mudança | Motivo |
|---|---|---|
| C-11 | **Mensagens diretas entram na v1** (D-01). RF-18 promovido a Must Have, com RF-18a e RF-18b. | Decisão do cliente. Implementação escolhida: reaproveitar a tabela `channels` com `guild_id` nulo e tipos `dm`/`group_dm`, mais uma tabela `channel_participants`. Isso reaproveita mensagens, anexos, reações e estado de leitura sem duplicação. A ressalva registrada na v1.1.0 permanece válida e foi endereçada nos dois pontos em que ela de fato morde: resolução de permissão (§5.3, passo 0) e segunda árvore de navegação (F4a). |
| C-12 | **Busca de mensagens entra na v1** (D-02). RF-17 promovido a Must Have; índice GIN ativado no schema. | Decisão do cliente. Viabilizada por C-02: em 500 MB o índice não caberia; em volume de 200 GB, cabe. O índice de trigrama fica desativado por padrão para não dobrar o custo de indexação sem necessidade comprovada. |
| C-13 | **A ponte com o Discord é permanente** (D-03). RF-31 passa a exigir fila persistente em tabela; novo RF-31a de reconciliação; nova tabela `bridge_outbox`. | Uma ponte transitória tolera perder mensagens numa queda, porque o Discord é desligado em semanas. Uma ponte permanente, não: fila em memória perde entregas a cada implantação (RNF-17), e sem reconciliação a divergência acumula silenciosamente. |
| C-14 | **Política de migração de mídia definida** (D-04): texto integral, imagens convertidas, vídeo por último e descartável. Novo RF-25a; `attachments.r2_key` passa a aceitar `NULL` com `skip_reason`. | Texto é o ativo insubstituível e é barato. Vídeo é o oposto: caro em armazenamento e sem valor de referência futura. Formalizar a ordem de execução garante que um estouro dos 10 GB do R2 nunca comprometa as camadas anteriores. |

---

## 1. Visão Geral do Sistema

### 1.1 Objetivo

Construir uma aplicação desktop privada para comunicação multimídia em tempo real — chat de texto persistente, voz, vídeo e compartilhamento de tela com áudio — para uma comunidade fechada de 10 a 30 pessoas, com migração completa do histórico de servidores legados do Discord e sincronização bidirecional contínua durante o período de transição.

O sistema deve operar sob custo operacional zero, hospedado em camada gratuita permanente.

### 1.2 Perfil de uso dimensionante

Estes números são premissa de projeto, não estimativa. Todo dimensionamento em §4 e §7 deriva deles.

| Dimensão | Valor |
|---|---|
| Usuários totais | 10 a 30 |
| Pico de usuários simultâneos em voz | 8 a 12 |
| Uso central | Watch party: 1 pessoa compartilha gameplay/estudo em 1080p60, 4 a 10 assistem, sessões longas |
| Publicadores de câmera simultâneos | 0 a 3 (uso secundário) |
| Plataforma dos clientes | Windows 10/11 x86_64 (100%) |
| Localização | Brasil (concentração no Nordeste) |
| Volume do histórico a migrar | Texto integral (custo desprezível). Imagens convertidas e comprimidas. Vídeo tratado como descartável — ver RF-25a |

### 1.3 Escopo do Produto

- **Cliente Desktop:** shell Tauri v2 (WebView2), UI em React 19 + TypeScript + Tailwind CSS.
- **Backend de Tempo Real & API:** servidor assíncrono em Rust (Axum + Tokio).
- **Infraestrutura de Mídia:** LiveKit SFU auto-hospedado na mesma VM.
- **Armazenamento de Objetos:** Cloudflare R2, com compressão WebP no cliente e URLs pré-assinadas.
- **Banco Relacional:** PostgreSQL 16+ auto-hospedado, acesso tipado via SQLx.
- **Ponte de Sincronização:** bot/relay bidirecional integrado ao gateway do Discord.

### 1.4 Fora de escopo na v1

Federação, criptografia ponta-a-ponta, clientes móveis, clientes web, threads, fóruns, eventos agendados, stage channels, bots de terceiros, monetização.

---

## 2. Arquitetura do Sistema

```
+---------------------------------------------------------------------------+
|                        TAURI V2 DESKTOP (Windows)                         |
|  +-------------------------------------+  +----------------------------+  |
|  |   Frontend UI (React 19 / TS / Vite)|  |  Core Nativo (Rust)        |  |
|  | - Virtualizacao (@tanstack/react-   |  | - Atalhos globais (PTT)    |  |
|  |   virtual)                          |  | - System tray              |  |
|  | - LiveKit JS SDK (WebRTC)           |  | - Notificacoes nativas     |  |
|  |   adaptiveStream + dynacast         |  | - Secure storage (keyring) |  |
|  | - Types gerados via ts-rs           |  | - IPC bridge               |  |
|  +-------------------------------------+  +----------------------------+  |
+------------------------------------+--------------------------------------+
                                     |
     +-------------------------------+-------------------------------+
     |                    |                    |                     |
     v                    v                    v                     v
[HTTPS / WSS]      [WebRTC / SRTP]     [TURN/TLS :5349]     [HTTPS presigned]
     |                    |                    |                     |
     |                    |                    |                     |
+----+--------------------+--------------------+----+     +----------+--------+
|          ORACLE CLOUD A1.Flex (ARM, 2 OCPU / 12 GB)|     |  CLOUDFLARE R2    |
|                        Regiao: Brasil              |     |  (anexos, avatars,|
|  +--------------------+  +----------------------+  |     |   dumps do DB)    |
|  |  Backend Axum      |  |  LiveKit SFU         |  |     +-------------------+
|  |  - REST API        |  |  - UDP 50000-60000   |  |
|  |  - WS Gateway      |  |  - TCP 7881          |  |
|  |  - Discord Bridge  |  |  - TURN/TLS 5349     |  |
|  |  - Webhooks LiveKit|<-+  - Webhooks -> Axum  |  |
|  +---------+----------+  +----------------------+  |
|            | TCP local (unix socket)               |
|  +---------v----------+   +---------------------+  |
|  |  PostgreSQL 16     |   |  Caddy (TLS 1.3)    |  |
|  |  volume 200 GB     |   |  :443 reverse proxy |  |
|  +--------------------+   +---------------------+  |
+-----------------------------------------------------+
                    |
                    v
          [ Discord Gateway / Webhooks ]
```

### 2.1 Decisões arquiteturais registradas (ADR resumido)

| ID | Decisão | Alternativa rejeitada | Justificativa |
|---|---|---|---|
| ADR-01 | Mídia inteiramente no LiveKit JS SDK dentro do WebView2 | SDK Rust nativo + IPC | Frames de vídeo não devem atravessar IPC (720p I420 a 30 fps ≈ 41 MB/s por stream). O SDK JS ainda entrega `adaptiveStream` e `dynacast` prontos, que são o mecanismo que mantém o egress dentro do orçamento. |
| ADR-02 | SFU auto-hospedado na mesma VM do backend | LiveKit Cloud | Ver C-01. A A1.Flex aloca 1 Gbps de banda por OCPU, então banda instantânea não é gargalo; o gargalo passa a ser egress mensal. |
| ADR-03 | Postgres na mesma VM | Supabase/Neon free tier | Ver C-02. Custo: backup e disponibilidade passam a ser responsabilidade própria (RNF-08). |
| ADR-04 | IDs UUIDv7 gerados na aplicação (`Uuid::now_v7()`) | UUIDv4 com `DEFAULT` no banco | Ordenação temporal natural, localidade de índice, paginação por keyset sem coluna auxiliar. |
| ADR-05 | Toda aquisição de mídia encapsulada em um único módulo (`src/media/tracks.ts`) | Chamadas diretas de `getDisplayMedia` nos componentes | Mantém aberta a porta para um sidecar nativo caso macOS/Linux entre em escopo, a custo zero hoje. |
| ADR-06 | Anexos em tabela própria, não em coluna JSONB | `messages.attachments JSONB` | O pipeline de espelhamento reescreve URLs e precisa de coleta de órfãos no R2 após deleções. |

---

## 3. Requisitos Funcionais (RF)

Marcadores: **[N]** novo em relação à v1.0.0 · **[M]** modificado em relação à v1.0.0. Não há mais requisitos pendentes de decisão.

### 3.1 Módulo 1 — Autenticação e Identidade

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-01 | Cadastro e login privado | Autenticação por e-mail e senha com hash Argon2id. Access token JWT de 15 min; refresh token opaco rotativo de 30 dias. | Must |
| RF-01a **[N]** | Rotação com detecção de reúso | Refresh tokens pertencem a uma *família*. Reapresentar um token já consumido revoga a família inteira e força novo login. | Must |
| RF-01b **[N]** | Armazenamento seguro do refresh token | O refresh token é persistido pelo core Rust via `tauri-plugin-stronghold` ou o cofre de credenciais do Windows, nunca em `localStorage`. | Must |
| RF-02 | Controle de acesso por convite | Criação de conta restrita a códigos de convite gerados por administradores, com limite de usos e expiração. | Must |
| RF-03 | Perfis de usuário | Apelido, avatar, bio e cor de destaque. | Should |
| RF-04 **[M]** | Presença em tempo real | Status Online / Ausente / Não Perturbar / Invisível propagado por WebSocket. Heartbeat a cada 30 s; ausência automática após 10 min sem input. **Invisível** é reportado a terceiros como Offline, mas o usuário continua recebendo eventos normalmente. | Must |

### 3.2 Módulo 2 — Servidores, Canais e Permissões

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-05 **[M]** | Estrutura de servidores | Guilds isolados contendo categorias colapsáveis e canais. Categorias são entidade persistida, com ordenação própria. | Must |
| RF-06 | Tipologia de canais | Canais de Texto (histórico persistente) e de Voz (áudio, vídeo, tela). | Must |
| RF-07 **[M]** | Cargos e permissões | RBAC com máscara de bits `BIGINT` (63 permissões disponíveis). Cargos por guild, com posição hierárquica e cargo padrão `@everyone` implícito. | Must |
| RF-07a **[N]** | Overwrites por canal | Permissões podem ser sobrescritas por canal, por cargo ou por membro, com pares allow/deny. Sem isso não existe canal privado. Algoritmo de resolução normativo em §5.3. | Must |
| RF-07b **[N]** | Gestão de membros | Entrada, saída, expulsão e banimento por guild, com apelido por guild. | Should |

### 3.3 Módulo 3 — Mensagens de Texto e Anexos

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-08 **[M]** | Envio e recebimento em tempo real | Publicação e entrega via gateway WebSocket assíncrono, sujeito às metas do RNF-01. Confirmação otimista no cliente com reconciliação por ID. | Must |
| RF-09 | Renderização de Markdown | Negrito, itálico, tachado, blockquotes, tabelas, listas, links e blocos de código com syntax highlighting. Parsing memoizado por mensagem (ver RNF-04). | Must |
| RF-10 | Upload via presigned URL | Upload direto do cliente para o R2 com URL pré-assinada emitida pelo Axum, sem passar bytes pelo backend. | Must |
| RF-11 **[M]** | Compressão de imagens no cliente | JPEG/PNG convertidos para WebP (qualidade 80, máx. 1920x1080) antes do envio. Dimensões originais persistidas junto ao anexo, para reserva de espaço na renderização. GIF, vídeo e outros tipos são enviados sem transcodificação, sujeitos ao limite do RF-11a. | Must |
| RF-11a **[N]** | Política de arquivos | Limite de 25 MB por arquivo e 10 arquivos por mensagem. Lista de tipos permitidos configurável. Recusa no backend antes de emitir a URL assinada. | Must |
| RF-12 **[M]** | Edição e exclusão | Edição registra `edited_at`; exclusão é lógica (`deleted_at`), preservando o mapeamento para propagação cruzada e idempotência. Coleta de órfãos no R2 em job diário. | Must |
| RF-13 | Indicador de digitação | Evento efêmero com expiração de 5 s, não persistido. | Should |
| RF-14 **[N]** | Respostas (reply) | Mensagem pode referenciar outra do mesmo canal, com preview do original. | Should |
| RF-15 **[N]** | Reações com emoji | Reações unicode agregadas por mensagem. Emojis customizados fora de escopo na v1. | Should |
| RF-16 **[N]** | Estado de não-lidas | Marcador de última mensagem lida por usuário e canal, contador de menções e separador visual "novas mensagens". Sem isso o produto não substitui o Discord na prática. | Must |
| RF-17 **[M]** | Busca de mensagens | Busca full-text em português (`to_tsvector('portuguese', …)` com índice GIN), filtrável por canal, autor e período. Escopo: guild atual ou conversa direta. Resultados paginados por keyset e filtrados por `VIEW_CHANNEL` no momento da consulta — nunca em cache. Índice de trigrama (`pg_trgm`) para busca parcial fica desativado por padrão e só é habilitado se a busca exata se mostrar insuficiente na prática. | Must |
| RF-18 **[M]** | Mensagens diretas | Conversas 1:1 e em grupo (limite de 10 participantes) fora de guilds. Persistidas na própria tabela `channels`, com `guild_id` nulo e tipo `dm`/`group_dm`, reaproveitando mensagens, anexos, reações, menções e estado de leitura sem duplicação de modelo. Conversa 1:1 entre um mesmo par é única: a aplicação resolve o canal existente em vez de criar um novo. | Must |
| RF-18a **[N]** | Acesso a conversas diretas | Conversas diretas não têm cargos nem overwrites. O acesso é determinado exclusivamente pela participação ativa em `channel_participants`, e a resolução de permissão faz curto-circuito (§5.3, passo 0). Ponte com o Discord é estruturalmente proibida neste tipo de canal. | Must |
| RF-18b **[N]** | Gestão de participantes em grupo | O criador pode adicionar e remover participantes; qualquer participante pode sair. Sair encerra o acesso à conversa; as mensagens permanecem visíveis para os demais. | Should |

> **Renumeração:** os RFs de mídia, migração, bridge e desktop foram deslocados em relação à v1.0.0 pela inserção de RF-14 a RF-18. O mapeamento antigo → novo está em §12.

### 3.4 Módulo 4 — Voz, Vídeo e Compartilhamento de Tela

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-19 **[M]** | Conexão a canais de voz | Entrada e saída de salas do LiveKit auto-hospedado, via token JWT emitido sob demanda pelo backend após verificação da permissão `CONNECT_VOICE`. | Must |
| RF-20 **[N]** | Estado de voz visível globalmente | A lista de participantes de cada canal de voz é visível para todos os membros do guild, inclusive quem não está conectado. Alimentada por webhooks do LiveKit (`participant_joined` / `participant_left`) recebidos pelo Axum e retransmitidos por WebSocket. | Must |
| RF-21 **[M]** | Transmissão de câmera | Vídeo de câmera com simulcast em 720p / 360p / 180p. Máximo de 3 publicadores simultâneos por sala (guard no backend, ver RNF-10). | Should |
| RF-22a **[M]** | Compartilhamento de tela inteira com áudio do sistema | Captura de tela inteira com áudio do sistema, 1080p a 30 ou 60 fps. `contentHint: 'motion'`, `degradationPreference: 'maintain-framerate'`. Camadas de simulcast explícitas: 1080p60 e 720p30. Codec VP9 (SVC) preferencial, H.264 como fallback. | Must |
| RF-22b **[M]** | Compartilhamento de janela específica | Captura de janela individual, **vídeo apenas**. O seletor da UI deve declarar explicitamente que áudio não está disponível nesta modalidade — é limitação do Chromium, não do produto. | Must |
| RF-22c **[N]** | Alerta de realimentação de áudio | Ao ativar áudio do sistema, exibir aviso de que fones de ouvido são necessários: a captura inclui a saída de áudio inteira, inclusive a voz dos demais participantes. Não há captura por aplicativo no Chromium. | Should |
| RF-23 **[M]** | Supressão de ruído e VAD | Cancelamento de eco e supressão de ruído via constraints padrão do `getUserMedia` (AEC3/NS do libwebrtc). VAD para indicador de "falando". Krisp/RNNoise adicional é opcional e fora do caminho crítico. | Should |

### 3.5 Módulo 5 — Migração e Ingestão do Histórico

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-24 **[M]** | Ingestão de JSON estruturado | Processamento em lote de exports do DiscordChatExporter, mapeando guilds, categorias, canais, autores e mensagens. Idempotente por `discord_message_id`: reexecutar não duplica. | Must |
| RF-24a **[N]** | Export obrigatoriamente via bot token | O export deve ser gerado com token de bot com permissão de leitura de histórico. Export via token de usuário caracteriza self-bot e viola os termos do Discord, com risco de banimento da conta. | Must |
| RF-25 **[M]** | Espelhamento de anexos | Download dos anexos e reenvio ao R2 antes da gravação no banco. **URLs de anexo do Discord são assinadas e expiram em poucas horas**, portanto o download deve ocorrer na janela de validade — preferencialmente usando o modo de download de assets do próprio exporter, no momento do export. | Must |
| RF-25a **[N]** | Política de migração por tipo de mídia | Ordem de execução **obrigatória**, para que estouro de cota nunca comprometa as camadas anteriores: **(1) Texto** — 100% migrado, sem exceção; é o ativo insubstituível e o custo é desprezível. **(2) Imagens** (JPEG/PNG/WebP) — convertidas para WebP qualidade 80, máx. 1920x1080, e enviadas ao R2. **(3) GIF** — migrados sem transcodificação enquanto houver orçamento; convertidos para WebP animado se necessário. **(4) Vídeo e áudio** — prioridade mínima, migrados apenas com folga confirmada nos 10 GB. Quando não migrados, o anexo é registrado como *placeholder* (`r2_key` nulo, `skip_reason` preenchido), preservando nome, tipo e tamanho originais, e a UI exibe "anexo não migrado". A mensagem e sua autoria são sempre preservadas. | Must |
| RF-26 | Ghost users | Criação de usuários legados com `is_migrated = true` e `discord_user_id` preenchido, sem e-mail nem senha, preservando a autoria visual. | Must |
| RF-26a **[N]** | Vinculação de identidade | Um usuário real pode vincular seu `discord_user_id`; ao vincular, as mensagens do ghost correspondente são reatribuídas em transação única. | Should |

### 3.6 Módulo 6 — Sincronização Bidirecional (Bridge)

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-27 **[M]** | Consumo de eventos do Discord | Escuta de `MESSAGE_CREATE`, `MESSAGE_UPDATE` e `MESSAGE_DELETE` via gateway, com bot oficial. Exige o intent privilegiado `MESSAGE_CONTENT` habilitado no portal de desenvolvedores. | Must |
| RF-28 **[M]** | Encaminhamento para o Discord | Mensagens originadas na aplicação são enviadas por webhook, sobrescrevendo `username` e `avatar_url`. O envio usa `?wait=true` para capturar o `discord_message_id` e permitir edição e deleção posteriores. | Must |
| RF-29 **[M]** | Mapeamento cruzado e anti-eco | Tabela `message_mappings` com `origin` explícito. Prevenção de loop em duas camadas: (1) descarte de eventos cujo `webhook_id` pertença à ponte; (2) verificação do mapeamento antes de reencaminhar. | Must |
| RF-30 **[N]** | Espelhamento de anexos ao vivo | Anexos que chegam pela ponte também são baixados e reenviados ao R2 no momento da ingestão. Sem isso, links de mensagens sincronizadas quebram em horas. | Must |
| RF-31 **[M]** | Fila persistente com controle de vazão | Como a ponte é permanente, a fila é persistida na tabela `bridge_outbox`, não apenas em memória: token bucket por webhook, respeito a `429`/`retry_after`, backoff exponencial e retomada após reinício sem perda nem duplicação. Uma implantação derruba o processo (RNF-17); a fila não pode morrer com ele. | Must |
| RF-31a **[N]** | Reconciliação periódica | Job diário que compara os últimos 7 dias de mensagens de cada canal em ponte contra `message_mappings` e reenvia o que ficou órfão por queda prolongada do gateway ou do webhook. Divergências não resolvidas são registradas para inspeção manual. Sem isso, uma ponte permanente acumula divergência silenciosa. | Should |
| RF-32 **[N]** | Escopo de ponte configurável | A ponte é habilitada por canal (`channels.bridge_enabled`), não globalmente. Canais internos novos não vazam para o Discord por padrão. | Must |

### 3.7 Módulo 7 — Integração Nativa Desktop

| ID | Requisito | Descrição | Prioridade |
|---|---|---|---|
| RF-33 **[M]** | Push-to-talk global | Hook de teclado global no Windows, ativo com a janela em segundo plano. Alterna `track.enabled` via evento IPC para o WebView. | Must |
| RF-34 | System tray | Execução em segundo plano, minimização para a bandeja, menu de contexto e restauração. | Should |
| RF-35 **[M]** | Notificações nativas | Notificações de menção direta, `@everyone` e resposta a mensagem própria, via subsistema do Windows. Suprimidas quando a janela está em foco no canal correspondente. | Should |
| RF-36 **[N]** | Atualização automática | Distribuição de instaladores `.msi` por GitHub Releases, com `tauri-plugin-updater` e verificação de assinatura. | Should |

---

## 4. Requisitos Não Funcionais (RNF)

### 4.1 Performance e latência

**RNF-01 (Latência de texto) [M].** Substitui a meta original de "<50 ms fim-a-fim", que não é atingível: só o RTT entre o Nordeste e o Sudeste consome 35–45 ms.
- p99 do tempo de processamento no gateway (recepção do frame WS → persistência → fan-out) < 15 ms, medido por histograma no `tracing`.
- p95 do tempo percebido (envio no cliente A → renderização no cliente B), em rede doméstica brasileira, < 150 ms.

**RNF-02 (Latência de mídia) [M].**
- Áudio boca-a-ouvido, p95 < 200 ms.
- Vídeo e tela, glass-to-glass, p95 < 300 ms.

**RNF-03 (Memória do cliente) [M].** Substitui 120/250 MB, inatingível: o WebView2 sozinho já parte de 80–150 MB.
- RSS agregado de todos os processos do aplicativo < 300 MB com chat ativo.
- < 700 MB durante watch party com uma tela 1080p60 e até duas câmeras.
- Medição: `Get-Process` somando a árvore de processos, após 30 min de uso contínuo.

**RNF-04 (Virtualização de UI) [M].** A meta de "60 fps fixos" é substituída por um protocolo de teste verificável:
- Cenário: canal com 100.000 mensagens, scroll programático a 4.000 px/s por 10 s.
- Critérios: p95 de frame time < 16,7 ms; nenhuma long task > 50 ms; número de nós de DOM da lista estável.
- Habilitadores obrigatórios: Markdown pré-parseado e memoizado por ID de mensagem; dimensões de imagem persistidas no banco para evitar reflow; paginação por keyset sobre `(channel_id, id)`.

### 4.2 Segurança e privacidade

**RNF-05 (Criptografia em trânsito).** TLS 1.3 obrigatório em HTTPS e WSS. Mídia sobre SRTP/DTLS. TURN sobre TLS na porta 5349.

**RNF-06 (Credenciais) [M].** Argon2id com memória 64 MB, iterações 3, paralelismo 4. Como cada verificação custa ~100 ms na VM ARM, o endpoint de login exige rate limit de 5 tentativas por minuto por IP e por conta.

**RNF-07 (Tokens de mídia).** JWT do LiveKit com escopo estrito de sala, expiração ≤ 60 min, renovado silenciosamente. O backend valida `CONNECT_VOICE` antes de emitir.

**RNF-15 (Segredos) [N].** Tokens de webhook do Discord, chaves do R2, segredo do LiveKit e chave de assinatura JWT vivem exclusivamente em variáveis de ambiente ou arquivo com permissão 600. Nunca no repositório, nunca em tabela sem criptografia.

**RNF-16 (LGPD) [N].** A migração torna o operador controlador de dados pessoais de terceiros. Requisitos mínimos: aviso prévio à comunidade antes do export; procedimento documentado de exclusão a pedido, cobrindo banco e R2; nenhum dado migrado exposto fora da aplicação.

### 4.3 Infraestrutura e custo zero

**RNF-08 (Persistência) [M].** Substitui integralmente o limite de 500 MB.
- Postgres auto-hospedado em volume de bloco Always Free (200 GB).
- Alvo de crescimento: < 40 GB em 24 meses, incluindo índices de busca.
- `pg_dump` comprimido diário para o R2, retenção de 14 dias, **com teste de restauração mensal registrado**. Backup não testado não é backup.

**RNF-09 (Objetos).** Entrega de mídia exclusivamente pelo R2, cujo egress é gratuito. Limites da camada gratuita a respeitar: 10 GB de armazenamento, 1 milhão de operações Classe A (escrita) e 10 milhões de Classe B (leitura) por mês. O pipeline de migração é intensivo em Classe A e deve ser executado em lotes monitorados, seguindo a ordem obrigatória do RF-25a: texto e imagens têm precedência absoluta sobre vídeo no consumo dos 10 GB.

**RNF-10 (Orçamento de egress) [M].** Substitui a gestão de cota de minutos da LiveKit Cloud.
- Teto: 6 TB/mês de saída, equivalente a 60% da franquia de 10 TB. Alerta em 70% do teto.
- Referência de consumo: uma tela 1080p60 a ~6 Mbps para 8 espectadores gera ~48 Mbps, ou ~21,6 GB/hora — cerca de 460 h/mês antes de esgotar a franquia total.
- Guards no backend: máximo de 3 publicadores de câmera por sala; desconexão de salas sem tráfego de áudio após 15 min; `adaptiveStream` e `dynacast` obrigatoriamente habilitados no cliente.

### 4.4 Portabilidade, confiabilidade e operação

**RNF-11 (Plataformas) [M].** A v1 suporta oficialmente Windows 10/11 x86_64. O código não deve conter dependências que impeçam compilação para macOS e Linux, mas essas plataformas não são testadas nem distribuídas na v1. Ver ADR-05.

**RNF-12 (Reconexão).** Backoff exponencial com jitter para o gateway WebSocket e para o WebRTC. Após reconectar, o cliente recupera o intervalo perdido por keyset a partir do último ID conhecido, sem recarregar o canal.

**RNF-13 (Portabilidade de infraestrutura) [N].** Toda a VM descrita como código (cloud-init + `docker compose` + configuração do Caddy), versionada no repositório. Objetivo de recuperação: migrar para um VPS pago equivalente em ≤ 2 horas, sem alteração no código da aplicação. Este requisito existe porque o provedor reduziu a cota Always Free de Ampere A1 de 4 OCPUs/24 GB para 2 OCPUs/12 GB em junho de 2026, sem anúncio público.

**RNF-14 (Observabilidade) [N].** Métricas mínimas expostas e coletadas: egress acumulado no mês, participantes por sala, latência de fan-out do WS, tamanho do banco, profundidade da fila da ponte, taxa de `429` do Discord. Logs estruturados em JSON via `tracing`, com retenção de 7 dias.

**RNF-17 (Implantação) [N].** A aplicação roda em instância única; não há fan-out entre processos. Consequência aceita: uma implantação derruba as conexões WebSocket ativas. Mitigada pelo RNF-12, com janela de deploy fora do horário de pico.

---

## 5. Modelo de Dados Relacional

### 5.1 Convenções normativas

- Todas as PKs são `UUID` v7 **geradas na aplicação** (`Uuid::now_v7()`). O banco não define `DEFAULT` para IDs; em PostgreSQL 18+, `uuidv7()` pode ser adotado como default de conveniência.
- Ordenação temporal de mensagens usa a própria PK. Paginação é sempre por keyset sobre `(channel_id, id)`, nunca `OFFSET`.
- IDs do Discord são `BIGINT` (snowflakes), sempre `NULL`-áveis e `UNIQUE` quando presentes.
- Exclusões de mensagem são lógicas. Exclusões de entidades estruturais são físicas, com `ON DELETE CASCADE`.

### 5.2 Schema

```sql
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- ============ IDENTIDADE ============

CREATE TABLE users (
    id               UUID PRIMARY KEY,
    email            VARCHAR(255),
    username         VARCHAR(32)  NOT NULL,
    display_name     VARCHAR(64),
    password_hash    VARCHAR(255),
    avatar_url       TEXT,
    accent_color     VARCHAR(7),
    bio              VARCHAR(500),
    discord_user_id  BIGINT UNIQUE,
    is_migrated      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ
);
-- Ghost users nao tem email nem senha; contas reais exigem ambos.
CREATE UNIQUE INDEX idx_users_email     ON users (lower(email))    WHERE email IS NOT NULL;
CREATE UNIQUE INDEX idx_users_username  ON users (lower(username)) WHERE is_migrated = FALSE;
ALTER TABLE users ADD CONSTRAINT chk_real_user_credentials
    CHECK (is_migrated = TRUE OR (email IS NOT NULL AND password_hash IS NOT NULL));

CREATE TABLE invites (
    id           UUID PRIMARY KEY,
    code         VARCHAR(16) UNIQUE NOT NULL,
    created_by   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    guild_id     UUID,
    max_uses     INT NOT NULL DEFAULT 1,
    uses         INT NOT NULL DEFAULT 0,
    expires_at   TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Familia de refresh tokens: reuso de um token consumido revoga a familia inteira.
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

-- ============ ESTRUTURA ============

CREATE TABLE guilds (
    id                UUID PRIMARY KEY,
    name              VARCHAR(100) NOT NULL,
    icon_url          TEXT,
    owner_id          UUID NOT NULL REFERENCES users(id),
    discord_guild_id  BIGINT UNIQUE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE guild_members (
    guild_id   UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id)  ON DELETE CASCADE,
    nickname   VARCHAR(64),
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    banned_at  TIMESTAMPTZ,
    PRIMARY KEY (guild_id, user_id)
);
CREATE INDEX idx_members_user ON guild_members (user_id);

-- permissions: mascara de 63 bits. Constantes em §5.3.
CREATE TABLE roles (
    id           UUID PRIMARY KEY,
    guild_id     UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name         VARCHAR(64) NOT NULL,
    color        VARCHAR(7),
    position     INT    NOT NULL DEFAULT 0,
    permissions  BIGINT NOT NULL DEFAULT 0,
    is_default   BOOLEAN NOT NULL DEFAULT FALSE,  -- cargo @everyone
    hoist        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_roles_default ON roles (guild_id) WHERE is_default;

CREATE TABLE member_roles (
    guild_id  UUID NOT NULL,
    user_id   UUID NOT NULL,
    role_id   UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (guild_id, user_id, role_id),
    FOREIGN KEY (guild_id, user_id) REFERENCES guild_members(guild_id, user_id) ON DELETE CASCADE
);

CREATE TABLE categories (
    id        UUID PRIMARY KEY,
    guild_id  UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name      VARCHAR(100) NOT NULL,
    position  INT NOT NULL DEFAULT 0
);

CREATE TYPE channel_type AS ENUM ('text', 'voice', 'dm', 'group_dm');

CREATE TABLE channels (
    id                  UUID PRIMARY KEY,
    guild_id            UUID REFERENCES guilds(id) ON DELETE CASCADE,  -- NULL em dm/group_dm
    category_id         UUID REFERENCES categories(id) ON DELETE SET NULL,
    name                VARCHAR(100) NOT NULL,
    topic               VARCHAR(1024),
    type                channel_type NOT NULL DEFAULT 'text',
    position            INT NOT NULL DEFAULT 0,
    discord_channel_id  BIGINT UNIQUE,
    bridge_enabled      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Canal de guild exige guild_id; conversa direta exige a ausencia dele.
    CONSTRAINT chk_channel_scope CHECK (
        (type IN ('text', 'voice')  AND guild_id IS NOT NULL) OR
        (type IN ('dm', 'group_dm') AND guild_id IS NULL AND category_id IS NULL)
    ),
    -- Ponte com Discord nunca se aplica a conversa direta (RF-18a).
    CONSTRAINT chk_bridge_scope CHECK (bridge_enabled = FALSE OR type = 'text')
);
CREATE INDEX idx_channels_guild ON channels (guild_id, position) WHERE guild_id IS NOT NULL;

-- Participantes de conversas diretas. Define acesso sozinho: sem cargos, sem overwrites.
-- Unicidade de DM 1:1 e garantida na aplicacao, resolvendo o canal existente
-- pelo par canonico de user_id antes de criar um novo.
CREATE TABLE channel_participants (
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    added_by    UUID REFERENCES users(id),
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    left_at     TIMESTAMPTZ,
    PRIMARY KEY (channel_id, user_id)
);
CREATE INDEX idx_participants_user ON channel_participants (user_id) WHERE left_at IS NULL;

CREATE TYPE overwrite_target AS ENUM ('role', 'member');

CREATE TABLE channel_overwrites (
    channel_id   UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    target_type  overwrite_target NOT NULL,
    target_id    UUID   NOT NULL,
    allow        BIGINT NOT NULL DEFAULT 0,
    deny         BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (channel_id, target_type, target_id)
);

-- ============ MENSAGENS ============

CREATE TABLE messages (
    id           UUID PRIMARY KEY,                 -- UUIDv7: ordena por tempo
    channel_id   UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    author_id    UUID NOT NULL REFERENCES users(id),
    content      TEXT NOT NULL DEFAULT '',         -- vazio e valido: mensagem so-anexo
    reply_to_id  UUID REFERENCES messages(id) ON DELETE SET NULL,
    is_pinned    BOOLEAN NOT NULL DEFAULT FALSE,
    edited_at    TIMESTAMPTZ,
    deleted_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_messages_channel     ON messages (channel_id, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_messages_author      ON messages (author_id);
CREATE INDEX idx_messages_pinned      ON messages (channel_id) WHERE is_pinned;
-- Busca full-text (RF-17). Custo: 30-50% do tamanho da tabela, ja previsto no RNF-08.
CREATE INDEX idx_messages_fts ON messages
    USING GIN (to_tsvector('portuguese', content)) WHERE deleted_at IS NULL;
-- Trigrama (busca parcial/typo) desativado por padrao: dobraria o custo de indice.
-- Habilitar so se a busca exata se mostrar insuficiente em uso real.
-- CREATE INDEX idx_messages_trgm ON messages
--     USING GIN (content gin_trgm_ops) WHERE deleted_at IS NULL;

CREATE TABLE attachments (
    id            UUID PRIMARY KEY,
    message_id    UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    r2_key        TEXT,             -- NULL = anexo nao migrado (placeholder, RF-25a)
    skip_reason   VARCHAR(32),      -- ex.: 'video_fora_do_orcamento', 'cdn_expirada'
    filename      VARCHAR(255) NOT NULL,
    content_type  VARCHAR(100) NOT NULL,
    size_bytes    BIGINT NOT NULL,
    width         INT,     -- persistido p/ reservar espaco e evitar reflow (RNF-04)
    height        INT,
    source_url    TEXT,    -- URL original do Discord, apenas p/ auditoria da migracao
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_attachments_message ON attachments (message_id);

CREATE TABLE reactions (
    message_id  UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    emoji       VARCHAR(32) NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (message_id, user_id, emoji)
);

CREATE TABLE mentions (
    message_id    UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id       UUID REFERENCES users(id) ON DELETE CASCADE,
    role_id       UUID REFERENCES roles(id) ON DELETE CASCADE,
    is_everyone   BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX idx_mentions_user ON mentions (user_id);

CREATE TABLE read_states (
    user_id               UUID NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    channel_id            UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    last_read_message_id  UUID,
    mention_count         INT NOT NULL DEFAULT 0,
    muted                 BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, channel_id)
);

-- ============ VOZ ============
-- UNLOGGED: estado efemero, nao precisa sobreviver a crash e nao gera WAL.
CREATE UNLOGGED TABLE voice_states (
    user_id     UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    session_id  TEXT NOT NULL,
    self_mute   BOOLEAN NOT NULL DEFAULT FALSE,
    self_deaf   BOOLEAN NOT NULL DEFAULT FALSE,
    streaming   BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_voice_channel ON voice_states (channel_id);

-- ============ PONTE DISCORD ============

CREATE TYPE message_origin AS ENUM ('internal', 'discord');

CREATE TABLE message_mappings (
    internal_message_id  UUID PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    discord_message_id   BIGINT UNIQUE NOT NULL,
    channel_id           UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    origin               message_origin NOT NULL,
    synced_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_mappings_discord ON message_mappings (discord_message_id);

-- Token do webhook NAO e armazenado aqui em texto puro: ver RNF-15.
CREATE TABLE channel_webhooks (
    channel_id          UUID PRIMARY KEY REFERENCES channels(id) ON DELETE CASCADE,
    discord_webhook_id  BIGINT NOT NULL,
    token_ref           TEXT NOT NULL,   -- referencia a variavel de ambiente/cofre
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fila persistente de saida da ponte (RF-31). Sobrevive a reinicio do processo.
CREATE TABLE bridge_outbox (
    id             UUID PRIMARY KEY,
    channel_id     UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    message_id     UUID REFERENCES messages(id) ON DELETE CASCADE,
    action         VARCHAR(16) NOT NULL,   -- create | update | delete
    payload        JSONB NOT NULL,
    attempts       INT NOT NULL DEFAULT 0,
    next_retry_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at   TIMESTAMPTZ,
    last_error     TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_outbox_pending ON bridge_outbox (next_retry_at) WHERE delivered_at IS NULL;
```

### 5.3 Permissões: constantes e algoritmo de resolução

Máscara de 63 bits em `BIGINT`. Bits reservados a partir de 20 para expansão futura.

| Bit | Constante |
|---|---|
| 0 | `ADMINISTRATOR` |
| 1 | `MANAGE_GUILD` |
| 2 | `MANAGE_ROLES` |
| 3 | `MANAGE_CHANNELS` |
| 4 | `KICK_MEMBERS` |
| 5 | `BAN_MEMBERS` |
| 6 | `CREATE_INVITE` |
| 7 | `VIEW_CHANNEL` |
| 8 | `SEND_MESSAGES` |
| 9 | `MANAGE_MESSAGES` |
| 10 | `ATTACH_FILES` |
| 11 | `EMBED_LINKS` |
| 12 | `ADD_REACTIONS` |
| 13 | `MENTION_EVERYONE` |
| 14 | `CONNECT_VOICE` |
| 15 | `SPEAK` |
| 16 | `VIDEO` |
| 17 | `SCREEN_SHARE` |
| 18 | `MUTE_MEMBERS` |
| 19 | `MOVE_MEMBERS` |

Ordem de resolução **normativa** (implementar exatamente assim; é fonte recorrente de erro):

```
0. Se channel.type e 'dm' ou 'group_dm':
     participante ativo em channel_participants (left_at IS NULL) ->
         VIEW_CHANNEL | SEND_MESSAGES | ATTACH_FILES | EMBED_LINKS |
         ADD_REACTIONS | CONNECT_VOICE | SPEAK | VIDEO | SCREEN_SHARE
     caso contrario -> 0 (nenhuma permissao)
   FIM. Cargos e overwrites nao se aplicam a conversas diretas.
1. Se o usuario e owner do guild -> todas as permissoes. FIM.
2. base = permissions do cargo @everyone
3. base |= OR das permissions de todos os cargos do membro
4. Se base contem ADMINISTRATOR -> todas as permissoes. FIM.
5. ow = overwrite do canal para o cargo @everyone
   base = (base & ~ow.deny) | ow.allow
6. Acumular todos os overwrites de cargo aplicaveis ao membro:
     role_deny  = OR dos deny
     role_allow = OR dos allow
   base = (base & ~role_deny) | role_allow
7. ow = overwrite do canal para o membro especifico
   base = (base & ~ow.deny) | ow.allow
8. Retornar base.
```

Um canal privado é, portanto, um canal com `deny VIEW_CHANNEL` no overwrite de `@everyone` e `allow VIEW_CHANNEL` nos cargos ou membros autorizados.

### 5.4 Estimativa de crescimento

| Item | Custo unitário estimado | Observação |
|---|---|---|
| Linha em `messages` (curta) | ~170 B | inclui overhead de tupla |
| Índice `idx_messages_channel` | ~45 B/linha | |
| PK + demais índices | ~60 B/linha | |
| **Total por mensagem** | **~280 B** | |
| 1 milhão de mensagens | ~280 MB | confortável em 200 GB |
| Índice GIN de busca (RF-17, ativo) | +30–50% da tabela | Já contabilizado no alvo de 40 GB do RNF-08 |

---

## 6. Fluxos Críticos

### 6.1 Anti-eco da ponte (normativo)

```
Mensagem nasce no app                    Mensagem nasce no Discord
        |                                          |
   INSERT messages                         MESSAGE_CREATE recebido
        |                                          |
   fan-out WS local                        webhook_id pertence a ponte?
        |                                          |-- sim -> DESCARTAR
   canal tem bridge_enabled?                       |-- nao -> segue
        |-- nao -> FIM                             |
        |-- sim                            discord_message_id ja mapeado?
        |                                          |-- sim -> DESCARTAR
   enfileirar POST webhook ?wait=true              |-- nao -> segue
        |                                          |
   resposta traz discord_message_id        resolver/criar ghost user
        |                                          |
   INSERT message_mappings                 espelhar anexos -> R2 (RF-30)
     (origin='internal')                           |
        |                                   INSERT messages
       FIM                                         |
                                            INSERT message_mappings
                                              (origin='discord')
                                                   |
                                             fan-out WS local
```

Edição e deleção seguem o mesmo par de guardas, consultando `message_mappings` em ambas as direções antes de propagar.

### 6.2 Upload de anexo

```
Cliente comprime p/ WebP -> POST /attachments/presign {filename, size, type}
  -> backend valida RF-11a e permissao ATTACH_FILES
  -> retorna PUT assinado + r2_key
Cliente faz PUT direto no R2
  -> POST /messages {content, attachments:[{r2_key, width, height, ...}]}
  -> backend confirma existencia do objeto (HEAD) antes de persistir
```

Objetos com `r2_key` sem linha correspondente em `attachments` após 24 h são removidos por job diário.

---

## 7. Topologia de Infraestrutura e Matriz de Custos

| Componente | Solução | Limite gratuito | Estratégia |
|---|---|---|---|
| Backend, SFU, DB, proxy | Oracle Cloud A1.Flex (ARM) | 2 OCPU / 12 GB (reduzido de 4/24 em jun/2026); 200 GB de bloco; ~1 Gbps por OCPU; 10 TB de egress/mês | Conta convertida para Pay As You Go — evita recuperação por ociosidade sem gerar cobrança dentro dos limites |
| Banco de dados | PostgreSQL 16 auto-hospedado | 200 GB de volume | Dump diário para o R2 (RNF-08) |
| Mídia SFU | LiveKit OSS auto-hospedado | Limitado por egress, não por minutos | `adaptiveStream` + `dynacast` + guards do RNF-10 |
| Objetos | Cloudflare R2 | 10 GB, 1M Classe A, 10M Classe B, egress zero | WebP obrigatório; migração em lotes monitorados |
| Distribuição | GitHub Releases | Ilimitado | Instalador `.msi` assinado, atualização automática |
| Domínio e TLS | Cloudflare DNS + Caddy (ACME) | Gratuito | TLS 1.3 automático |

### 7.1 Armadilhas operacionais conhecidas

1. **iptables da imagem base.** Liberar portas na Security List da VCN não basta: as imagens Ubuntu e Oracle Linux da OCI trazem regras locais de firewall que bloqueiam tudo além da 22. Ajustar `iptables`/`nftables` no cloud-init.
2. **Home region imutável.** A região de origem da tenancy não pode ser alterada após a criação da conta. Escolher uma região no Brasil antes de qualquer coisa.
3. **"Out of capacity" de A1.** Provisionamento de Ampere A1 falha com frequência; prever script de retry contra a API `LaunchInstance`.
4. **Portas do LiveKit.** UDP 50000–60000, TCP 7881, TURN/TLS 5349. Sem TURN em 5349 com certificado válido, usuários atrás de CGNAT não conectam.

---

## 8. Matriz de Riscos

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Provedor reduz ou remove a cota Always Free novamente | Média | Alto | RNF-13: infraestrutura como código; migração para VPS pago em ≤ 2 h |
| Instância recuperada por ociosidade (CPU no p95 abaixo do limiar por 7 dias) | Média | Alto | Conversão da conta para Pay As You Go |
| Indisponibilidade de capacidade A1 na região escolhida | Alta | Médio | Provisionar cedo; script de retry |
| Rate limit de webhook do Discord | Alta | Médio | RF-31: fila com token bucket e respeito a `retry_after` |
| Expiração das URLs assinadas da CDN do Discord | Certeza | Alto | RF-25 e RF-30: download e reenvio ao R2 dentro da janela de validade |
| CGNAT bloqueando mídia | Média | Alto | TURN/TLS obrigatório em 5349 |
| Realimentação de áudio no screen share | Alta | Médio | RF-22c: aviso e exigência de fones |
| Perda do histórico migrado (VM única, sem HA) | Baixa | Crítico | RNF-08: dump diário fora do provedor + teste de restauração mensal |
| Banimento da conta Discord por uso de self-bot no export | Baixa | Alto | RF-24a: export exclusivamente por bot token |
| VM única satura CPU durante watch party e degrada a API | Baixa | Médio | Limitar recursos por `systemd` slice / `docker compose`; RNF-14 para detectar |

---

## 9. Roadmap por Fatias Verticais

Cada fatia entrega valor observável e tem critério de aceite executável.

| # | Fatia | Aceite |
|---|---|---|
| F0 | Infraestrutura como código: cloud-init, compose (Postgres + LiveKit + Caddy), CI com build do `.msi` | `curl https://.../health` responde; deploy do zero reproduzível |
| F1 | **Spike de maior risco:** screen share 1080p60 com áudio do sistema entre duas máquinas Windows, contra o SFU auto-hospedado | Sessão de 30 min estável, egress medido, latência glass-to-glass registrada |
| F2 | Autenticação: convites, cadastro, login, rotação de refresh, armazenamento seguro | Suíte de testes cobrindo detecção de reúso de token |
| F3 | Estrutura: guilds, categorias, canais, cargos, overwrites, resolução de permissão | Testes unitários do algoritmo §5.3 com casos de canal privado |
| F4 | Chat em guild: WS gateway, envio, edição, exclusão, virtualização, não-lidas | Protocolo do RNF-04 executado com 100k mensagens semeadas |
| F4a **[N]** | Conversas diretas 1:1 e em grupo: participantes, curto-circuito de permissão, segunda árvore de navegação | Teste que prova que não participante recebe 403 e não aparece no fan-out do WS |
| F4b **[N]** | Busca full-text: índice GIN, filtros, paginação por keyset | Busca em canal sem `VIEW_CHANNEL` não retorna resultado, mesmo com termo exato |
| F5 | Anexos: presign, WebP, política de arquivos, coleta de órfãos | Upload e render de imagem sem reflow |
| F6 | Voz e tela integradas na UI: estado de voz global, PTT, guards de egress | Watch party real com 6 pessoas |
| F7 **[M]** | Migração: ingestão idempotente, ghost users, espelhamento de anexos por camada (RF-25a) | Reexecução do mesmo export não duplica linhas; vídeo não migrado renderiza como placeholder sem quebrar a mensagem |
| F8 **[M]** | Ponte bidirecional permanente: consumo, encaminhamento, anti-eco, `bridge_outbox`, reconciliação | Teste de loop (mensagem enviada nos dois lados não se replica) **e** teste de reinício: processo morto com fila cheia entrega tudo ao voltar, sem duplicar |

F1 vem antes de tudo que não seja infraestrutura porque é o requisito de maior risco técnico e maior valor percebido: se ele não fechar dentro do orçamento de egress, o produto muda de forma.

---

## 10. Decisões Fechadas

| ID | Decisão | Resolução | Onde foi implementada |
|---|---|---|---|
| **D-01** | Mensagens diretas entram na v1? | **Sim.** 1:1 e em grupo, limite de 10 participantes. | RF-18, RF-18a, RF-18b; `channels.guild_id` nulo + `channel_participants`; §5.3 passo 0; F4a |
| **D-02** | Busca de mensagens entra na v1? | **Sim.** Full-text em português, com trigrama desativado por padrão. | RF-17; `idx_messages_fts`; §5.4; F4b |
| **D-03** | A ponte é permanente ou transitória? | **Permanente.** | RF-31 (fila persistente), RF-31a (reconciliação); tabela `bridge_outbox`; F8 |
| **D-04** | Política de volume da migração | **Texto integral; imagens convertidas e comprimidas; vídeo despriorizado e descartável.** | RF-25a; `attachments.r2_key` nulo + `skip_reason`; RNF-09; F7 |

### 10.1 Pontos derivados a confirmar durante a implementação

Nenhum destes bloqueia o início do desenvolvimento; todos têm padrão definido e podem ser ajustados sem retrabalho estrutural.

| ID | Ponto | Padrão adotado | Revisar quando |
|---|---|---|---|
| P-01 | Limite de participantes em conversa de grupo | 10 | Se o uso mostrar demanda por grupos maiores |
| P-02 | Índice de trigrama para busca parcial | Desativado | Se a busca exata gerar reclamação recorrente de "não achei" |
| P-03 | Canais de voz em conversas diretas | Permitidos (o passo 0 de §5.3 já concede `CONNECT_VOICE`) | Se a UI de chamada direta se mostrar cara em F4a; é rebaixável a Should |
| P-04 | Destino dos vídeos não migrados | Placeholder com metadados; arquivo descartado | Se o export couber com folga nos 10 GB, migrar também na passada final |
| P-05 | Janela de reconciliação da ponte | 7 dias | Após medir a frequência real de queda do gateway |

---

## 11. Convenções de Desenvolvimento em Harness de IA

O código de aplicação será majoritariamente escrito em Claude Code. Os itens abaixo não são preferência de estilo: são pré-condições para que a geração seja confiável.

1. **`SQLX_OFFLINE=true` com `.sqlx/` versionado.** As macros do SQLx validam queries em tempo de compilação contra um banco vivo. Sem o cache offline (`cargo sqlx prepare`) o agente não compila e passa a "consertar" o problema trocando `query!` por `query`, perdendo a checagem de tipos silenciosamente. Adicionar `cargo sqlx prepare --check` ao CI.
2. **Comando único de verificação.** `just check` = `cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo sqlx prepare --check` + `cargo test` + `tsc --noEmit` + `vitest run`. O agente precisa de um oráculo de correção que ele mesmo possa executar.
3. **Contrato primeiro.** Tipos definidos em Rust são fonte única de verdade; `ts-rs` gera os tipos TypeScript no build. Nenhum tipo de payload escrito à mão no frontend.
4. **`CLAUDE.md` na raiz e por crate**, contendo: proibição de `unwrap`/`expect` em handlers, enum de erro único com `IntoResponse`, spans de `tracing` obrigatórios em handlers, paginação sempre por keyset, e o algoritmo de §5.3 transcrito.
5. **Zonas proibidas ao agente sem revisão humana:** migrations destrutivas, código de captura de mídia e permissões de SO, credenciais e configuração de rede da VM.
6. **Fixtures determinísticos:** um export reduzido do DiscordChatExporter e um fake do gateway do Discord, para que F7 e F8 sejam testáveis sem tocar na API real.
7. **Testes de integração com banco real** (`testcontainers` ou Postgres em compose no CI), não mocks de repositório: o valor está em validar o SQL.

---

## 12. Mapeamento de Renumeração v1.0.0 → v1.1.0

| v1.0.0 | v1.1.0 | Nota |
|---|---|---|
| RF-01 a RF-13 | inalterados | RF-04, RF-05, RF-07, RF-08, RF-11, RF-12 modificados |
| RF-14 | RF-19 | |
| RF-15 | RF-21 | |
| RF-16 | RF-22a e RF-22b | dividido |
| RF-17 | RF-23 | |
| RF-18 | RF-24 | |
| RF-19 | RF-25 | |
| RF-20 | RF-26 | |
| RF-21 | RF-27 | |
| RF-22 | RF-28 | |
| RF-23 | RF-29 | |
| RF-24 | RF-33 | |
| RF-25 | RF-34 | |
| RF-26 | RF-35 | |
| RNF-01 a RNF-12 | mesma numeração | RNF-01, 02, 03, 04, 06, 08, 10, 11 modificados |
| — | RNF-13 a RNF-17 | novos |
