> ### ⚠️ Parcialmente obsoleto — reescrita pendente na fatia S1
>
> Escrito para o produto da v1, substituído em 2026-09-12 pelo complemento de screen share
> — ver [ADR-0008](adr/0008-complemento-ao-discord.md).
>
> **Continua válido, e é o que há de melhor no projeto:** o mecanismo do gateway inteiro —
> `HELLO`/`IDENTIFY`/`RESUME`, sequência, buffer de retomada, batimento, códigos de
> fechamento, limites, e o princípio de que o WS é quase unidirecional. Nada disso muda.
>
> **Morto, sai em S1:** cerca de 22 das 30 variantes de dispatch (mensagens, reações,
> digitação, DMs, cargos, categorias, canais, estado de leitura, ponte).
>
> **Sobrevive:** `READY`, `RESUMED`, `PRESENCE_UPDATE` e `VOICE_STATE_UPDATE` — este
> último já carrega `streaming`, que é exatamente o sinal de "alguém está compartilhando".
>
> **Ainda não escrito:** os eventos de sala e de revogação ao vivo
> ([ADR-0010](adr/0010-autorizacao-derivada-do-discord.md)).
>
> Campos aditivos não incrementam a versão do protocolo; a poda de eventos, sim.

---

# Protocolo do Gateway WebSocket

**Versão do protocolo:** 1
**Endpoint:** `wss://<host>/gateway?v=1`
**Status:** normativo para as partes que sobrevivem — ver aviso acima.

Este documento é o contrato entre `crates/api` (gateway) e `desktop/src/gateway` (cliente). Os tipos de payload vivem em `crates/protocol` e são gerados para TypeScript via `just types` — **não escreva payload à mão em nenhum dos dois lados**.

---

## 1. Princípio: o WebSocket é quase unidirecional

O cliente envia **apenas** três coisas: `IDENTIFY`, `RESUME` e `HEARTBEAT`. Todo o resto — enviar mensagem, editar, reagir, sinalizar digitação, marcar como lido — é REST.

Isso é deliberado. Escrita via WebSocket exigiria duplicar validação, autorização, rate limit e tratamento de erro em dois caminhos, e é a origem clássica de divergência de comportamento entre eles. O gateway é um canal de **notificação**, e o REST é o canal de **mutação**.

Consequência prática: toda mutação bem-sucedida no REST produz um ou mais eventos de dispatch no gateway, inclusive para quem originou a ação.

---

## 2. Envelope

Todo frame, nos dois sentidos, é JSON com este formato:

```json
{ "op": 0, "t": "MESSAGE_CREATE", "s": 4211, "d": { } }
```

| Campo | Tipo | Presença |
|---|---|---|
| `op` | inteiro | sempre |
| `t` | string | apenas em `op: 0` — nome do evento |
| `s` | inteiro | apenas em `op: 0` — sequência da sessão, monotônica, começa em 1 |
| `d` | objeto | payload; ausente onde não se aplica |

Frames que não seguem o envelope são descartados e a conexão é encerrada com código `4002`.

### 2.1 Opcodes

| op | Nome | Direção | Descrição |
|---|---|---|---|
| 0 | `DISPATCH` | S→C | Evento de domínio. Sempre traz `t` e `s`. |
| 1 | `HELLO` | S→C | Primeiro frame após a conexão. Traz `heartbeat_interval_ms`. |
| 2 | `IDENTIFY` | C→S | Autenticação com access token. |
| 3 | `RESUME` | C→S | Retomada de sessão existente. |
| 4 | `HEARTBEAT` | C→S | Batimento. |
| 5 | `HEARTBEAT_ACK` | S→C | Confirmação do batimento. |
| 6 | `INVALID_SESSION` | S→C | Não é possível retomar. Reidentifique e recupere lacuna por REST. |
| 7 | `RECONNECT` | S→C | O servidor pede reconexão (implantação). A sessão continua retomável. |

---

## 3. Ciclo de vida da conexão

```
Cliente conecta
      |
      v
  <- HELLO { heartbeat_interval_ms: 30000, session_ttl_ms: 90000 }
      |
      +-- tem session_id e last_seq? --> RESUME ->
      |                                     |
      |                              sessao viva e sem lacuna?
      |                                 sim -> replay dos dispatches faltantes
      |                                        -> DISPATCH RESUMED
      |                                 nao -> INVALID_SESSION -> volta ao IDENTIFY
      |
      +-- caso contrario --> IDENTIFY { token } ->
                                    |
                             token valido?
                                nao -> close 4001
                                sim -> DISPATCH READY { session_id, user, guilds, ... }
```

### 3.1 `IDENTIFY`

```json
{ "op": 2, "d": { "token": "<access token JWT>", "client": { "version": "1.0.0", "os": "windows" } } }
```

O token é o mesmo access token do REST, com validade de 15 minutos. **A conexão não cai quando o token expira** — a autenticação é verificada na identificação. A expiração afeta apenas o REST, e o cliente renova por lá normalmente.

Resposta: `DISPATCH` com `t: "READY"`.

```json
{
  "op": 0, "t": "READY", "s": 1,
  "d": {
    "session_id": "01J8...",
    "user": { },
    "guilds": [ { "id": "...", "name": "...", "channels": [], "roles": [], "member_count": 0 } ],
    "dm_channels": [ ],
    "read_states": [ { "channel_id": "...", "last_read_message_id": "...", "mention_count": 0 } ],
    "presences": [ ],
    "voice_states": [ ]
  }
}
```

`READY` traz a **estrutura**, não o conteúdo: canais, cargos, membros, estado de leitura, presença e estado de voz. Mensagens vêm por REST, sob demanda, ao abrir cada canal. Um `READY` que carregasse histórico tornaria a inicialização O(n) no tamanho do servidor.

`READY` só inclui canais em que o usuário tem `VIEW_CHANNEL` no momento da identificação.

### 3.2 Batimento

O cliente envia `{ "op": 4 }` a cada `heartbeat_interval_ms` (30 s, alinhado ao RF-04), com jitter aleatório de até 10% para evitar sincronização de rebanho. O servidor responde `{ "op": 5 }`.

Se o cliente não receber `HEARTBEAT_ACK` após dois intervalos, considera a conexão zumbi, fecha com código `4900` e reconecta com `RESUME`.
Se o servidor não receber `HEARTBEAT` após dois intervalos, encerra a conexão e marca a sessão como retomável pelo `session_ttl_ms`.

O batimento é também o que mantém a presença: ausência de batimento leva o usuário a offline após o TTL.

### 3.3 `RESUME`

```json
{ "op": 3, "d": { "token": "<access token>", "session_id": "01J8...", "last_seq": 4211 } }
```

O servidor mantém, por sessão, um buffer circular dos **últimos 500 dispatches** por até `session_ttl_ms` (90 s) após a desconexão.

- Se a sessão existe e `last_seq` está dentro do buffer: reenvia os dispatches faltantes na ordem original, preservando os valores de `s`, e finaliza com `DISPATCH RESUMED`.
- Se a sessão expirou ou a lacuna é maior que o buffer: `{ "op": 6, "d": { "resumable": false } }`. O cliente então faz `IDENTIFY` e **recupera a lacuna por REST**, buscando por keyset a partir do último `message.id` conhecido em cada canal aberto (RNF-12).

Buffer de 500 e TTL de 90 s cobrem oscilação de rede doméstica e reinício de processo. Não cobrem hibernação da máquina — e não devem: nesse caso a recuperação por REST é mais barata que manter estado no servidor.

### 3.4 `RECONNECT` e implantação

O sistema roda em instância única (RNF-17), então toda implantação derruba as conexões. Antes de encerrar, o servidor envia `{ "op": 7 }` a todas as sessões. O cliente deve reconectar imediatamente com `RESUME`, sem backoff na primeira tentativa — o TTL de 90 s dá margem para o processo voltar.

### 3.5 Códigos de fechamento

| Código | Significado | Cliente deve |
|---|---|---|
| 4000 | Erro desconhecido | Retomar |
| 4001 | Autenticação inválida | Renovar token e reidentificar; se falhar de novo, deslogar |
| 4002 | Frame malformado | Reidentificar (é bug do cliente; registrar) |
| 4003 | Não identificado a tempo (10 s) | Reidentificar |
| 4004 | Sessão já ativa em outra conexão com o mesmo `session_id` | Reidentificar |
| 4008 | Excesso de frames (rate limit) | Backoff e reidentificar |
| 4900 | Fechamento iniciado pelo cliente (zumbi) | Retomar |

### 3.6 Reconexão

Backoff exponencial com jitter: 1s, 2s, 4s, 8s, 16s, teto de 30s, jitter de ±30%. Contador zera após uma sessão que durou mais de 60 s. Exceção: `RECONNECT` (op 7) tenta imediatamente uma vez antes de entrar no backoff.

---

## 4. Fan-out: quem recebe o quê

**Esta é a seção mais fácil de implementar errado.** Um evento nunca é transmitido para "o guild"; ele é transmitido para o conjunto de sessões cujo usuário tem permissão de ver aquele canal, calculada no momento do envio.

### 4.1 Cálculo do conjunto de destinatários

| Tipo de canal | Destinatários |
|---|---|
| `text`, `voice` | Membros do guild com `VIEW_CHANNEL` resolvido para aquele canal (SRS §5.3) |
| `dm`, `group_dm` | Participantes ativos em `channel_participants` (`left_at IS NULL`). Cargos e overwrites não se aplicam. |

Eventos que não pertencem a um canal (presença, membro, cargo) vão para os membros do guild correspondente.

### 4.2 Cache de permissão no gateway

Calcular a resolução completa de permissão a cada mensagem é caro. O gateway mantém, por guild, um índice em memória de `channel_id -> Set<user_id>` com quem enxerga cada canal.

O índice é **invalidado obrigatoriamente** por: alteração de cargo, de atribuição de cargo, de overwrite de canal, criação ou remoção de canal, e entrada ou saída de membro. Quando invalidado, o gateway emite `PERMISSIONS_STALE` para os afetados (§5).

Este cache é a única exceção autorizada à regra 7 do `CLAUDE.md`, e vale **somente para roteamento de notificação**. Qualquer resposta que carregue conteúdo — REST ou busca — recalcula a permissão na consulta. O pior caso do cache desatualizado é uma notificação a mais; o pior caso no REST seria vazamento de conteúdo.

### 4.3 Múltiplas sessões

Um usuário pode ter várias sessões. Todas recebem os mesmos eventos, inclusive as originadas por ele mesmo. É o que sincroniza estado de leitura e presença entre máquinas.

---

## 5. Eventos de dispatch

Nomes em `SCREAMING_SNAKE_CASE`. Campos em `snake_case`. Todo payload de entidade segue o mesmo formato usado no REST.

### Sessão
| `t` | `d` | Notas |
|---|---|---|
| `READY` | ver §3.1 | |
| `RESUMED` | `{ "replayed": 12 }` | |

### Mensagens
| `t` | `d` | Notas |
|---|---|---|
| `MESSAGE_CREATE` | objeto Message completo, com `nonce` quando aplicável | `nonce` só é enviado à sessão que originou; para as demais vem `null` |
| `MESSAGE_UPDATE` | Message completo | Reenvia o objeto inteiro, não um diff. Diff economiza bytes irrelevantes e custa bugs de merge. |
| `MESSAGE_DELETE` | `{ "id", "channel_id" }` | Exclusão é lógica no banco; para o cliente é remoção |
| `MESSAGE_BULK_DELETE` | `{ "ids": [], "channel_id" }` | Usado pela migração e pela moderação |

### Reações e digitação
| `t` | `d` |
|---|---|
| `REACTION_ADD` | `{ "message_id", "channel_id", "user_id", "emoji" }` |
| `REACTION_REMOVE` | idem |
| `TYPING_START` | `{ "channel_id", "user_id", "expires_at" }` |

`TYPING_START` é efêmero e nunca persistido. Não entra no buffer de resume: retomar um indicador de digitação de 40 segundos atrás é ruído.

### Estrutura
| `t` | `d` |
|---|---|
| `CHANNEL_CREATE` / `CHANNEL_UPDATE` / `CHANNEL_DELETE` | objeto Channel |
| `CATEGORY_CREATE` / `CATEGORY_UPDATE` / `CATEGORY_DELETE` | objeto Category |
| `ROLE_CREATE` / `ROLE_UPDATE` / `ROLE_DELETE` | objeto Role |
| `GUILD_MEMBER_ADD` / `GUILD_MEMBER_UPDATE` / `GUILD_MEMBER_REMOVE` | objeto Member |
| `PERMISSIONS_STALE` | `{ "guild_id" }` |

`PERMISSIONS_STALE` avisa o cliente que sua visão de permissões pode estar errada e ele deve recarregar a estrutura do guild por REST. É emitido depois de qualquer alteração em cargo ou overwrite. Sem ele, um usuário rebaixado continua vendo botões que o servidor vai recusar.

### Presença e voz
| `t` | `d` |
|---|---|
| `PRESENCE_UPDATE` | `{ "user_id", "status" }` — `status` ∈ `online \| idle \| dnd \| offline` |
| `VOICE_STATE_UPDATE` | `{ "user_id", "channel_id", "self_mute", "self_deaf", "streaming" }` · `channel_id: null` = saiu |

`PRESENCE_UPDATE` nunca revela `invisible`: quem está invisível é reportado a terceiros como `offline`, e só a si mesmo como `invisible` (RF-04).

`VOICE_STATE_UPDATE` é alimentado pelos webhooks do LiveKit recebidos pelo backend, **não pelo cliente** — é o que faz o estado de voz ser visível para quem não está na sala (RF-20). O cliente é fonte apenas para `self_mute` e `self_deaf`, via REST.

### Conversas diretas e leitura
| `t` | `d` |
|---|---|
| `DM_CHANNEL_CREATE` | objeto Channel com `participants` |
| `DM_PARTICIPANT_ADD` / `DM_PARTICIPANT_REMOVE` | `{ "channel_id", "user_id" }` |
| `READ_STATE_UPDATE` | `{ "channel_id", "last_read_message_id", "mention_count" }` |

`READ_STATE_UPDATE` é enviado só às sessões do próprio usuário. É o que mantém as não-lidas coerentes entre máquinas.

### Ponte
| `t` | `d` |
|---|---|
| `BRIDGE_STATUS` | `{ "connected": bool, "queue_depth": int, "last_error": string \| null }` |

Enviado apenas a administradores, em mudança de estado ou a cada 60 s enquanto houver falha. Uma ponte permanente que cai em silêncio é pior que uma ponte que não existe.

---

## 6. Ordenação e consistência

1. **`s` é monotônico por sessão**, e serve só para detectar lacuna no resume. Não é ordem global de eventos.
2. **A ordem de exibição de mensagens é dada por `message.id`** (UUIDv7, ordenável por tempo), nunca pela ordem de chegada. O cliente insere ordenado por ID.
3. **Não há entrega exatamente-uma-vez.** Após resume, um evento pode chegar duas vezes. Todo handler do cliente é idempotente: aplicar `MESSAGE_CREATE` duas vezes com o mesmo ID resulta em uma mensagem.
4. **Eventos podem se referir a entidades que o cliente desconhece** (mensagem de um canal que ele nunca abriu). O cliente ignora silenciosamente em vez de buscar: o canal será carregado quando o usuário o abrir.
5. **O contador de não-lidas é do servidor.** O cliente exibe o que vem em `READ_STATE_UPDATE`; não recalcula localmente, para não divergir entre máquinas.

---

## 7. Limites e proteções

| Limite | Valor | Ação ao exceder |
|---|---|---|
| Tempo para identificar após `HELLO` | 10 s | close 4003 |
| Frames recebidos do cliente | 30 por 60 s | close 4008 |
| Tamanho de frame recebido | 4 KB | close 4002 |
| Conexões simultâneas por usuário | 4 | recusa a mais antiga |
| Buffer de resume por sessão | 500 dispatches | descarte do mais antigo |
| TTL de sessão desconectada | 90 s | sessão descartada |

O limite de 30 frames por minuto é folgado para um cliente que só envia batimento a cada 30 s. Um cliente que o atinge está com defeito.

---

## 8. Versionamento

A versão vai no query string (`?v=1`). Mudança compatível (campo novo, evento novo) não incrementa a versão — clientes ignoram o que não conhecem. Mudança incompatível (campo removido, semântica alterada) incrementa, e o servidor mantém a versão anterior por pelo menos um ciclo de release.

O cliente envia sua versão em `IDENTIFY.d.client.version`. O servidor responde com `close 4010` se ela estiver abaixo do mínimo suportado, e o app dispara o fluxo de atualização automática (RF-36).

---

## 9. Checklist de implementação

Servidor (`crates/api/src/gateway/`):
- [ ] Envelope e opcodes com serialização de `crates/protocol`
- [ ] Registro de sessão com buffer circular e TTL
- [ ] Batimento com detecção de zumbi nos dois sentidos
- [ ] Índice `channel_id -> Set<user_id>` com invalidação nos cinco gatilhos de §4.2
- [ ] Emissão de `PERMISSIONS_STALE`
- [ ] `RECONNECT` no desligamento gracioso
- [ ] Limites de §7

Cliente (`desktop/src/gateway/`):
- [ ] Máquina de estados: `connecting → identifying → ready → resuming → closed`
- [ ] Backoff com jitter e exceção do `RECONNECT`
- [ ] Recuperação de lacuna por REST após `INVALID_SESSION`
- [ ] Handlers idempotentes por ID
- [ ] Reconciliação de `nonce` para envio otimista
