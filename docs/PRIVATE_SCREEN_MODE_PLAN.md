# Plano de implementação — chamada privada 1:1

Este documento detalha e acompanha a implementação da fatia **S10**, aceita pelo
[ADR-0036](adr/0036-chamadas-privadas-coexistem-com-canais-discord.md). O código-base da
fatia está implementado; o ensaio ponta a ponta em duas máquinas continua sendo o portão
de entrega antes do PR.

## Resultado esperado

Duas pessoas, sem estarem num servidor ou canal de voz do Discord, conseguem:

1. autenticar o aplicativo com a própria conta do Discord;
2. criar uma chamada e obter um código curto de uso único;
3. entrar na mesma chamada informando o código;
4. publicar e assistir tela com os recursos de mídia já existentes;
5. encerrar a chamada e desconectar as duas pontas.

O modo atual continua igual: `/tela` pareia o aplicativo, e entrar ou sair de um canal de
voz continua dirigindo a sala daquele servidor.

## Fora do MVP

- link de convite;
- expulsão individual;
- mais de duas pessoas;
- lista, busca ou histórico de chamadas;
- convite renovável ou segundo convidado;
- microfone, câmera, chat e contatos;
- WebRTC ponto a ponto;
- login por e-mail, senha ou outro provedor;
- gravação ou persistência de mídia.

## Critério de aceite ponta a ponta

Em duas máquinas que não estão num canal de voz de guild:

1. A autentica por OAuth e cria uma chamada;
2. B autentica por OAuth e consome o código mostrado por A;
3. A compartilha uma janela e B recebe o primeiro quadro em menos de 3 segundos após a
   publicação;
4. B também consegue publicar uma tela na mesma chamada;
5. reutilizar o código, usar código expirado ou tentar entrar como terceira pessoa falha;
6. A encerra a chamada e as duas conexões são removidas do LiveKit em menos de 5 segundos;
7. nenhum dos dois consegue obter novo token para a chamada encerrada;
8. os testes do pareamento `/tela`, da réplica e das salas de canal continuam verdes.

## Modelo mínimo

### Login OAuth

Adicionar `oauth_login_attempts` para o retorno seguro do navegador ao aplicativo:

- `id UUID PRIMARY KEY`, gerado com UUIDv7;
- hashes separados do `state` OAuth e do segredo de consulta do aplicativo;
- `user_id UUID NULL` após o callback concluir;
- `expires_at`, `consumed_at` e `created_at`.

Fluxo:

1. `POST /auth/discord/start` cria a tentativa e devolve `authorize_url`, `attempt_id` e um
   segredo de consulta;
2. o navegador autoriza somente `identify`;
3. `GET /auth/discord/callback` valida `state`, troca o código no backend, consulta
   `/users/@me`, faz upsert do usuário e conclui a tentativa;
4. `POST /auth/discord/complete` recebe `attempt_id` e o segredo, consome a tentativa e
   emite o par de tokens interno já existente;
5. access e refresh tokens do Discord são descartados depois de `/users/@me`.

Aplicar TTL curto, consumo único, comparação em tempo constante, rate limit e mensagens de
erro sem revelar se um `attempt_id` existe.

### Chamada privada

Usar uma única tabela `private_calls`, sem abstração genérica de membros:

- `id UUID PRIMARY KEY`, gerado com UUIDv7;
- `owner_id UUID NOT NULL REFERENCES users(id)`;
- `guest_id UUID NULL REFERENCES users(id)`;
- `invite_hash CHAR(64) UNIQUE NOT NULL`;
- `invite_expires_at TIMESTAMPTZ NOT NULL`;
- `invite_consumed_at TIMESTAMPTZ NULL`;
- `created_at TIMESTAMPTZ NOT NULL`;
- `ended_at TIMESTAMPTZ NULL`.

Entrar é uma única transação: localizar o hash ainda válido, exigir `guest_id IS NULL`,
gravar o convidado e consumir o convite. Essa escrita é o limite real de duas pessoas e
fecha a corrida entre dois consumidores.

O código precisa ter ao menos 128 bits de entropia antes da representação amigável. Só o
hash SHA-256 vai para o banco.

## Contratos REST e tempo real

Adicionar DTOs em `crates/protocol` e gerar TypeScript com `just types`.

| Método | Rota | Função |
|---|---|---|
| `POST` | `/private-calls` | cria chamada e devolve o código uma única vez |
| `POST` | `/private-calls/join` | consome o código e devolve o estado da chamada |
| `GET` | `/private-calls/{id}` | recupera estado após reconexão |
| `POST` | `/private-calls/{id}/token` | emite token LiveKit após validar membro e chamada ativa |
| `DELETE` | `/private-calls/{id}` | dono encerra e desconecta os participantes |

O gateway continua somente como notificação. Introduzir eventos específicos, sem tornar
`discord_channel_id` opcional nos eventos atuais:

- `PRIVATE_CALL_JOIN`: informa ao dono que o convidado entrou;
- `PRIVATE_CALL_END`: tira ambos da chamada e informa o motivo;
- eventos de participante e compartilhamento próprios do modo privado, somente se a UI
  não puder derivá-los com segurança dos eventos do LiveKit.

Toda publicação é dirigida aos dois IDs da chamada; nunca fazer broadcast global.

## Admissão e LiveKit

O módulo LiveKit passa a reconhecer duas chaves reais de sala:

- canal: `dvc-<discord_channel_id>`;
- privada: `private-<private_call_id>`.

O guard de publicadores deve ser indexado pela chave completa da sala, não apenas por
`discord_channel_id`. Para chamada privada, `owner_id` e `guest_id` ativos podem assistir e
publicar. Para canal, nada muda na resolução de permissões.

Ao encerrar:

1. gravar `ended_at` antes de qualquer chamada externa;
2. recusar imediatamente novas emissões de token;
3. remover do LiveKit as identidades normais e as identidades `~pub` dos dois membros;
4. emitir `PRIVATE_CALL_END` para as sessões conectadas;
5. tratar remoção já ocorrida como sucesso idempotente.

Webhooks de sala `private-*` não devem consultar a réplica do Discord. No MVP, também não
alimentam `share_sessions`, cujo schema representa canais do Discord; contabilização de
egress privado fica para uma fatia posterior, sem falsificar o modelo atual.

## Cliente desktop

Quando autenticado e fora de canal de voz, a tela ociosa ganha somente duas ações:

- **Criar chamada**;
- **Entrar com código**.

O store passa a representar a origem da sessão como união discriminada, mantendo os tipos
atuais de canal intactos:

```ts
type ActiveSession =
  | { kind: 'discord_voice'; room: RoomState }
  | { kind: 'private_call'; call: PrivateCallState };
```

`media/session.ts` recebe a chave e a rota de token da sessão ativa. Depois da obtenção do
token, captura, publicação, preview, tracks, grade, foco, volume e áudio seguem pelo mesmo
caminho existente.

O fluxo OAuth abre o navegador, mostra estado de espera cancelável e consulta a tentativa
até sucesso, expiração ou cancelamento. Nenhum token fica em `localStorage`; o refresh token
interno continua no cofre do Windows.

## Ordem de implementação e testes

### 1. Aceitar a decisão e congelar contratos

- revisar o ADR-0036 e mudar seu status para `Aceito`;
- documentar REST e WebSocket antes de implementar;
- adicionar os tipos Rust e gerar os tipos TypeScript;
- escrever testes de serialização do wire.

### 2. OAuth sem remover pareamento

- adicionar configuração e validação de boot;
- criar migration e repositório de tentativas;
- implementar start, callback e complete;
- testar expiração, `state` inválido, consumo duplo e identidade já pareada;
- confirmar que `/auth/pair` e refresh token continuam passando.

### 3. Persistência e concorrência da chamada

- criar migration aditiva de `private_calls`;
- implementar criação, entrada transacional, leitura e encerramento idempotente;
- testar dois convidados consumindo o mesmo código em paralelo: exatamente um entra;
- testar dono, convidado, terceiro, expiração e chamada encerrada com PostgreSQL real.

### 4. Admissão LiveKit

- adicionar nome e parser de sala privada;
- generalizar a chave interna do guard de publicadores;
- emitir token somente para os dois membros;
- desconectar espectador e publicador ao encerrar;
- cobrir tokens e webhooks com testes e fixtures.

### 5. Gateway e recuperação

- entregar eventos apenas ao dono e ao convidado;
- incluir chamada privada ativa no caminho de recuperação após reconnect/resume;
- testar gap, resume e encerramento com cliente desconectado;
- não alterar o fan-out das salas Discord.

### 6. Interface e execução ponta a ponta

- implementar entrada OAuth, criar, entrar por código e encerrar;
- adaptar store e sessão de mídia pela origem discriminada;
- testar estados de espera, erro, expiração e reconexão;
- executar o critério de aceite em duas máquinas;
- rodar `just check` e registrar números do ensaio.

## Arquivos com impacto esperado

- `migrations/`: duas migrations aditivas;
- `crates/protocol/src/`: OAuth, chamada privada e eventos;
- `crates/db/src/repo/`: tentativas OAuth e chamadas;
- `crates/api/src/routes/`: autenticação e chamadas;
- `crates/api/src/livekit.rs`: chave, token, parser e desconexão;
- `crates/api/src/gateway/`: fan-out e recuperação;
- `desktop/src/app/runtime.ts`: OAuth e seleção do modo;
- `desktop/src/api/` e tipos gerados;
- `desktop/src/store/`: sessão ativa discriminada;
- `desktop/src/media/session.ts`: rota de token conforme a origem;
- `desktop/src/features/`: tela ociosa, criação e entrada por código;
- `docs/rest-api.md`, `docs/websocket.md`, `docs/DECISIONS.md` e SRS.

Não há alteração prevista em captura, preview, áudio ou encoder dentro de
`desktop/src-tauri/`.

## Portão de conclusão

A fatia só está pronta quando o aceite em duas máquinas passa, `just check` fica verde, o
cache `.sqlx/` e os tipos TypeScript estão atualizados, e o fluxo `/tela` existente passa
sem regressão. OAuth configurado incorretamente deve parar o modo privado com erro claro,
sem derrubar o modo de servidor.
