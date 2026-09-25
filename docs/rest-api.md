> ### ⚠️ Parcialmente obsoleto — reescrita pendente na fatia S1
>
> Este contrato foi escrito para o produto da v1 (chat, DMs, busca, anexos), substituído em
> 2026-09-12 pelo complemento de screen share — ver
> [ADR-0008](adr/0008-complemento-ao-discord.md).
>
> **Continua válido:** convenções gerais (§1), formato de erro (§3), paginação por keyset,
> e o contrato de `/auth/refresh`, `/auth/logout` e das rotas de voz/LiveKit.
>
> **Morto, sai em S1:** mensagens, reações, fixados, estado de leitura, DMs, busca, anexos,
> convites, guilds, cargos, categorias, overwrites, `/auth/register`, `/auth/login`, e toda
> a seção de administração da ponte.
>
> **Ainda não escrito:** pareamento por código
> ([ADR-0009](adr/0009-identidade-por-pareamento.md)) e as rotas de sala atreladas ao
> snowflake do Discord ([ADR-0011](adr/0011-sala-e-o-canal-de-voz.md)).
>
> Norma de escopo: [`SRS-v2.0-complemento-screen-share.md`](SRS-v2.0-complemento-screen-share.md).

> **Adendo S10 (normativo):** além do pareamento `/auth/pair`, o modo privado usa
> `POST /auth/discord/start`, `GET /auth/discord/callback` e
> `POST /auth/discord/complete`. O OAuth pede somente `identify`; o token do Discord é
> descartado após `/users/@me`. Chamadas privadas autenticadas usam:
>
> | Método | Rota | Função |
> |---|---|---|
> | `POST` | `/private-calls` | cria a chamada e revela uma vez o código de convite |
> | `POST` | `/private-calls/join` | consome o código de uso único |
> | `GET` | `/private-calls/{id}` | recupera o estado para dono ou convidado |
> | `POST` | `/private-calls/{id}/token` | emite token LiveKit para um membro da chamada ativa |
> | `DELETE` | `/private-calls/{id}` | permite ao dono encerrar a chamada |

---

# Contrato da API REST

**Base:** `https://<host>/api/v1`
**Status:** normativo para as partes que sobrevivem — ver aviso acima.

O REST é o canal de **mutação e recuperação**; o WebSocket é o canal de **notificação**. Toda mutação bem-sucedida aqui produz o dispatch correspondente descrito em `docs/protocol/websocket.md` §5. Se um endpoint muda estado e não tem evento correspondente, é bug de especificação — pergunte antes de implementar.

---

## 1. Convenções gerais

- `Content-Type: application/json; charset=utf-8` em toda requisição e resposta com corpo, exceto o upload direto ao R2.
- Campos em `snake_case`. IDs são strings contendo UUIDv7. **Nunca trate ID como número.**
- Timestamps em RFC 3339 com fuso (`2026-08-29T14:03:22.481Z`).
- Toda resposta traz `X-Request-Id`. Todo relato de erro deve citá-lo.
- Ausência de campo e `null` significam a mesma coisa em resposta. Em `PATCH`, campo ausente = não alterar, `null` = limpar.

## 2. Autenticação

`Authorization: Bearer <access_token>` em tudo, exceto `/auth/register`, `/auth/login`, `/auth/refresh` e `/health`.

- Access token: JWT, 15 min, `sub` = user_id, `jti` para revogação.
- Refresh token: opaco, 30 dias, rotativo com detecção de reúso (RF-01a). Armazenado pelo core Rust no cofre do Windows — **nunca** em `localStorage`.
- Ao receber `401` com `code: "TOKEN_EXPIRED"`, o cliente chama `/auth/refresh` uma vez e repete a requisição original. Duas falhas seguidas = deslogar.

## 3. Formato de erro

Único formato, para todo erro, sem exceção:

```json
{
  "error": {
    "code": "FORBIDDEN",
    "message": "Você não tem permissão para enviar mensagens neste canal.",
    "details": null,
    "request_id": "01J8XQ..."
  }
}
```

- `code`: `SCREAMING_SNAKE_CASE`, estável, usado em lógica de cliente.
- `message`: português, exibível ao usuário. Nunca contém detalhe interno, stack trace ou SQL.
- `details`: só em `VALIDATION_FAILED`, com `[{ "field": "username", "code": "TOO_LONG" }]`.

| HTTP | `code` | Quando |
|---|---|---|
| 400 | `VALIDATION_FAILED` | Corpo ou query inválidos |
| 401 | `UNAUTHENTICATED` / `TOKEN_EXPIRED` / `TOKEN_REUSED` | `TOKEN_REUSED` revogou a família inteira; forçar login |
| 403 | `FORBIDDEN` | Recurso **visível**, ação negada |
| 404 | `NOT_FOUND` | Inexistente **ou invisível** |
| 409 | `CONFLICT` | Ex.: username em uso, convite já consumido |
| 413 | `PAYLOAD_TOO_LARGE` | Acima do limite do RF-11a |
| 429 | `RATE_LIMITED` | Com header `Retry-After` |
| 500 | `INTERNAL` | Nunca detalha; só `request_id` |

> **Regra de vazamento:** se o usuário não tem `VIEW_CHANNEL` no canal, responda **404**, jamais 403. Um 403 confirma que o canal existe e quem está nele — o suficiente para mapear a estrutura de um servidor privado. A regra vale para canal, mensagem, anexo e resultado de busca.

## 4. Paginação por keyset

**`OFFSET` é proibido** (`CLAUDE.md` §2.1). Toda coleção ordenável usa:

```
GET /channels/{id}/messages?before=<message_id>&limit=50
GET /channels/{id}/messages?after=<message_id>&limit=50
GET /channels/{id}/messages?around=<message_id>&limit=50
```

- `before` / `after` são exclusivos e mutuamente exclusivos entre si.
- `around` retorna `limit/2` de cada lado, para abrir um canal em uma mensagem específica (salto por busca ou por resposta).
- `limit`: padrão 50, máximo 100.
- Resposta ordenada por `id` **decrescente** (mais recente primeiro), exceto com `after`.

```json
{ "data": [ ], "has_more": true }
```

Sem `total`. Contagem exata sobre 100k mensagens é uma varredura por página, e ninguém usa o número.

## 5. Limites de taxa

`X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` em toda resposta sujeita a limite.

| Escopo | Limite |
|---|---|
| Login e registro | 5 / min por IP e por conta |
| Envio de mensagem | 10 / 10 s por canal e por usuário |
| Presign de anexo | 20 / min por usuário |
| Busca | 20 / min por usuário |
| Emissão de token de voz | 10 / min por usuário |
| Global por usuário | 300 / min |

---

## 6. Endpoints

Legenda de permissão: a permissão exigida (SRS §5.3). `—` = só autenticação.

### 6.1 Autenticação e conta

| Método | Rota | Perm. | Notas |
|---|---|---|---|
| `POST` | `/auth/register` | — | `{ invite_code, email, username, password }`. Consome o convite na mesma transação. |
| `POST` | `/auth/login` | — | Retorna `{ access_token, refresh_token, expires_in, user }` |
| `POST` | `/auth/refresh` | — | Rotativo: invalida o token apresentado e emite um novo par. Reúso → 401 `TOKEN_REUSED` e revogação da família |
| `POST` | `/auth/logout` | — | Revoga a família de refresh atual |
| `GET` | `/users/@me` | — | |
| `PATCH` | `/users/@me` | — | `display_name`, `avatar_url`, `bio`, `accent_color` |
| `PATCH` | `/users/@me/presence` | — | `{ status }`. É a única fonte de `idle`/`dnd`/`invisible`; `online`/`offline` derivam do batimento |
| `POST` | `/users/@me/discord-link` | — | Vincula `discord_user_id` e reatribui mensagens do ghost (RF-26a). Transacional |
| `GET` | `/users/{id}` | — | Perfil público |

### 6.2 Convites

| Método | Rota | Perm. |
|---|---|---|
| `POST` | `/invites` | `CREATE_INVITE` |
| `GET` | `/invites` | `MANAGE_GUILD` |
| `DELETE` | `/invites/{code}` | `MANAGE_GUILD` |
| `GET` | `/invites/{code}` | — (público, para pré-validar no cadastro; devolve só validade e nome do guild) |

### 6.3 Guilds, categorias, canais

| Método | Rota | Perm. |
|---|---|---|
| `GET` | `/guilds` | — (guilds do usuário) |
| `GET` | `/guilds/{id}` | `VIEW_CHANNEL` em ao menos um canal |
| `PATCH` | `/guilds/{id}` | `MANAGE_GUILD` |
| `GET` | `/guilds/{id}/members` | `VIEW_CHANNEL` |
| `PATCH` | `/guilds/{id}/members/{user_id}` | `MANAGE_ROLES` (cargos) ou próprio (apelido) |
| `DELETE` | `/guilds/{id}/members/{user_id}` | `KICK_MEMBERS` |
| `PUT` | `/guilds/{id}/bans/{user_id}` | `BAN_MEMBERS` |
| `POST`/`PATCH`/`DELETE` | `/guilds/{id}/categories[/{cid}]` | `MANAGE_CHANNELS` |
| `POST` | `/guilds/{id}/channels` | `MANAGE_CHANNELS` |
| `PATCH`/`DELETE` | `/channels/{id}` | `MANAGE_CHANNELS` |
| `PATCH` | `/guilds/{id}/channels/positions` | `MANAGE_CHANNELS` — reordenação em lote, transacional |

### 6.4 Cargos e overwrites

| Método | Rota | Perm. |
|---|---|---|
| `GET`/`POST` | `/guilds/{id}/roles` | `MANAGE_ROLES` |
| `PATCH`/`DELETE` | `/guilds/{id}/roles/{rid}` | `MANAGE_ROLES` |
| `PUT` | `/channels/{id}/permissions/{target_type}/{target_id}` | `MANAGE_ROLES` — `{ allow, deny }` como strings decimais |
| `DELETE` | `/channels/{id}/permissions/{target_type}/{target_id}` | `MANAGE_ROLES` |

> Máscaras de bits trafegam como **string decimal**, não como número. `BIGINT` de 63 bits não sobrevive ao `Number` do JavaScript, que perde precisão acima de 2^53. Este é o bug clássico da categoria, e não é hipotético.

Toda alteração aqui emite `PERMISSIONS_STALE` e invalida o índice de fan-out do gateway.

### 6.5 Mensagens

| Método | Rota | Perm. |
|---|---|---|
| `GET` | `/channels/{id}/messages` | `VIEW_CHANNEL` |
| `POST` | `/channels/{id}/messages` | `SEND_MESSAGES` (+ `ATTACH_FILES` se houver anexo) |
| `PATCH` | `/channels/{id}/messages/{mid}` | autor |
| `DELETE` | `/channels/{id}/messages/{mid}` | autor ou `MANAGE_MESSAGES` |
| `PUT` | `/channels/{id}/messages/{mid}/pin` | `MANAGE_MESSAGES` |
| `GET` | `/channels/{id}/pins` | `VIEW_CHANNEL` |
| `PUT`/`DELETE` | `/channels/{id}/messages/{mid}/reactions/{emoji}/@me` | `ADD_REACTIONS` |
| `POST` | `/channels/{id}/typing` | `SEND_MESSAGES` — 204, sem corpo |
| `PUT` | `/channels/{id}/read-state` | `VIEW_CHANNEL` — `{ last_read_message_id }` |

`POST /messages`:

```json
{
  "content": "texto",
  "nonce": "01J8XQ...",
  "reply_to_id": null,
  "attachments": [ { "r2_key": "...", "filename": "...", "content_type": "image/webp",
                     "size_bytes": 40213, "width": 1280, "height": 720 } ]
}
```

- `nonce` é gerado pelo cliente e devolvido no `MESSAGE_CREATE` **apenas à sessão de origem**, para reconciliar o envio otimista.
- `nonce` é **idempotente por 60 s**: repetir o mesmo `nonce` no mesmo canal retorna a mensagem já criada, com `200` em vez de `201`. É o que impede duplicata quando o cliente reenvia após timeout de rede.
- `content` vazio é válido se houver ao menos um anexo.
- O backend confirma a existência do objeto no R2 (`HEAD`) antes de persistir o anexo.
- Menções são extraídas no servidor a partir do conteúdo e gravadas em `mentions`. O cliente não envia lista de menções — seria confiável no cliente apenas até alguém abrir o DevTools.

### 6.6 Anexos

| Método | Rota | Perm. |
|---|---|---|
| `POST` | `/attachments/presign` | `ATTACH_FILES` no canal alvo |

```
POST /attachments/presign
{ "channel_id": "...", "filename": "captura.webp", "content_type": "image/webp", "size_bytes": 40213 }

201 { "r2_key": "att/01J8.../captura.webp", "upload_url": "https://...", "expires_in": 300 }
```

Valida o RF-11a (25 MB por arquivo, tipo permitido) **antes** de emitir a URL. O cliente faz `PUT` direto no `upload_url` e depois referencia o `r2_key` em `POST /messages`. Objetos sem mensagem associada após 24 h são removidos por job diário.

### 6.7 Conversas diretas

| Método | Rota | Perm. |
|---|---|---|
| `GET` | `/dms` | — |
| `POST` | `/dms` | — — `{ recipient_ids: [] }`. Com um destinatário, **resolve o canal existente** em vez de criar outro (RF-18) |
| `POST` | `/dms/{id}/participants` | criador do grupo |
| `DELETE` | `/dms/{id}/participants/{uid}` | criador, ou o próprio (sair) |

Máximo de 10 participantes. Todos os endpoints de mensagem de §6.5 funcionam igual para canais `dm` e `group_dm`; a resolução de permissão faz curto-circuito no passo 0 (SRS §5.3).

### 6.8 Busca

| Método | Rota | Perm. |
|---|---|---|
| `GET` | `/search` | avaliada por canal, no momento da consulta |

```
GET /search?q=texto&guild_id=...&channel_id=...&author_id=...&before=<id>&limit=25
```

- Escopo: `guild_id` **ou** `channel_id`, nunca busca global sem escopo.
- O conjunto de canais consultáveis é recalculado a cada requisição, com base na permissão atual. **Não use o índice de fan-out do gateway aqui.**
- Ordenação por `id` decrescente (recência), não por relevância. Em um servidor privado de 30 pessoas, "a mais recente que casa" é quase sempre o que se procura, e evita explicar ranking.
- Resposta traz a mensagem e os IDs vizinhos, para o cliente abrir o canal com `around`.

### 6.9 Voz

| Método | Rota | Perm. |
|---|---|---|
| `POST` | `/channels/{id}/voice-token` | `CONNECT_VOICE` |
| `PATCH` | `/voice-states/@me` | — — `{ self_mute, self_deaf }` |
| `POST` | `/internal/livekit/webhook` | assinatura do LiveKit, não Bearer |

`voice-token` emite JWT do LiveKit com escopo estrito de sala e validade ≤ 60 min (RNF-07). O cliente renova silenciosamente antes de expirar. Antes de emitir, o backend aplica os guards do RNF-10 (máximo de 3 publicadores de câmera por sala); ao recusar, responde `409` com `code: "VOICE_CAPACITY"`.

`/internal/livekit/webhook` é a **única** fonte de `VOICE_STATE_UPDATE`. Valide a assinatura e ignore corpo não assinado.

### 6.10 Administração e ponte

| Método | Rota | Perm. |
|---|---|---|
| `GET` | `/admin/bridge/status` | `ADMINISTRATOR` |
| `POST` | `/admin/bridge/channels/{id}` | `ADMINISTRATOR` — habilita ponte e cria o webhook |
| `DELETE` | `/admin/bridge/channels/{id}` | `ADMINISTRATOR` |
| `POST` | `/admin/bridge/reconcile` | `ADMINISTRATOR` — dispara RF-31a sob demanda |
| `GET` | `/health` | — |
| `GET` | `/metrics` | rede interna |

Habilitar ponte em canal `dm` ou `group_dm` retorna `409 CONFLICT`, e a `CHECK` constraint do banco é a segunda barreira.

---

## 7. Objetos

Os tipos autoritativos estão em `crates/protocol`. Este resumo existe para leitura; o TypeScript vem de `just types`.

```jsonc
// Message
{
  "id": "01J8...",
  "channel_id": "01J8...",
  "author": { "id", "username", "display_name", "avatar_url", "accent_color", "is_migrated" },
  "content": "…",
  "reply_to": { "id", "author_username", "excerpt" } | null,
  "attachments": [ {
      "id", "filename", "content_type", "size_bytes", "width", "height",
      "url": "https://…" | null,        // null = nao migrado (RF-25a)
      "skip_reason": "video_fora_do_orcamento" | null
  } ],
  "reactions": [ { "emoji", "count", "me": true } ],
  "is_pinned": false,
  "edited_at": null,
  "created_at": "2026-08-29T14:03:22.481Z",
  "nonce": null,
  "bridge": { "origin": "discord" } | null
}
```

```jsonc
// Channel
{
  "id", "guild_id": "…" | null, "category_id": "…" | null,
  "name", "topic", "type": "text|voice|dm|group_dm",
  "position", "bridge_enabled": false,
  "participants": [ /* so em dm e group_dm */ ],
  "permissions": "8796093022207"   // mascara efetiva do solicitante, string decimal
}
```

`channel.permissions` é a máscara **já resolvida** para quem pediu. O cliente usa esse campo para habilitar ou esconder controles; nunca reimplementa a resolução de §5.3 no frontend. O servidor revalida em toda mutação de qualquer forma.

---

## 8. Checklist de implementação

- [ ] Middleware de `request_id` + span de tracing em toda rota
- [ ] `AppError` como único tipo de erro em assinatura de handler
- [ ] 404 em vez de 403 para recurso invisível, em todas as rotas
- [ ] Máscaras de permissão serializadas como string decimal
- [ ] Idempotência de `nonce` com janela de 60 s
- [ ] Validação do RF-11a antes de emitir presign
- [ ] Verificação de assinatura no webhook do LiveKit
- [ ] Limites de §5 aplicados com headers de rate limit
- [ ] Todo endpoint de mutação com o dispatch correspondente no gateway
