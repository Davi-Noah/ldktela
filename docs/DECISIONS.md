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

- **[E3] Um container Postgres por binário de teste, um banco por teste** — subir um
  container por teste custava ~4 s cada. O container é criado num `OnceCell` e nunca
  descartado; o Docker o recolhe quando o processo termina. Alternativa descartada:
  transação por teste com rollback, que não funciona para testes de concorrência.
- **[E3] `resolve_for_channel` devolve `Option<Permissions>`** — `None` = canal inexistente,
  `Some(NONE)` = canal invisível. O repositório mantém os dois estados distintos; é a rota
  que os colapsa em 404 (`docs/api/rest-api.md` §3). Sem essa distinção não dá para logar
  a diferença sem vazá-la ao cliente.
- **[E3] Não-membro e membro banido resolvem para `NONE`** — o §5.3 pressupõe um membro e
  não trata o caso. Decidido no repositório, antes de aplicar o algoritmo.
- **[E3] `resolve_for_guild` separado, sem os passos 0 e 5–7** — rotas como `CREATE_INVITE`
  e `MANAGE_GUILD` não têm canal; um overwrite de canal não pode remover permissão de guild.
- **[E3] Todos os overwrites do canal são lidos numa consulta e filtrados em Rust** — um
  canal carrega poucos overwrites; três consultas separadas custariam mais que a filtragem.
- **[E3] O nonce de idempotência não vira coluna** — nada no SRS §5.2 o prevê, e uma coluna
  mais índice no caminho quente para uma janela de 60 s é caro. Vai para um mapa em memória
  no E7, coerente com a instância única do RNF-17. Descartada a heurística de deduplicar por
  (autor, canal, conteúdo, janela), que apagaria mensagens iguais enviadas de propósito.
- **[E3] `insert_guild_channel` recebe um `NewGuildChannel`** — oito argumentos posicionais
  é onde uma troca de `guild_id` por `category_id` passa despercebida, e o clippy recusa.

- **[E4] `argon2` fixado em 0.5, não 0.6** — a 0.6 reescreveu a API (`SaltString` saiu da
  raiz, `hash_password` perdeu o parâmetro de salt, `PasswordHash` virou alias depreciado).
  Corrige a linha de versões do E0. `jsonwebtoken` 11 exige feature de provider explícita:
  `default-features = false, features = ["rust_crypto"]`.
- **[E4] Hash do refresh token com SHA-256, não Argon2** — o token é 256 bits uniformes,
  não há dicionário a atacar. Argon2 custaria ~167 ms por refresh (medido) sem ganho.
- **[E4] O perdedor de uma corrida de rotação derruba a família** — duas rotações simultâneas
  com o mesmo token são indistinguíveis de um roubo. O viés correto é derrubar: o cliente
  honesto sempre pode logar de novo, o token roubado vira inútil.
- **[E4] Login gasta Argon2 mesmo para e-mail inexistente** (`verify_dummy`) — sem isso o
  tempo de resposta enumera contas. O `DUMMY_HASH` tem teste próprio provando que parseia,
  porque um PHC inválido faz `verify` retornar cedo e a defesa evapora em silêncio.
- **[E4] `Config::from_source` recusa Argon2 abaixo do RNF-06 e chave JWT com menos de 32
  caracteres** — os dois erros são invisíveis em runtime: logins continuam funcionando,
  só que baratos de quebrar.
- **[E4] `X-Request-Id` vindo do cliente só é aceito se for alfanumérico e tiver 8..64
  caracteres** — o id vai para linha de log; string arbitrária ali é injeção de log.
- **[E4] Fixtures de teste não compartilham `PgPool`** — `#[tokio::test]` cria um runtime
  por teste e um pool registra tarefa de manutenção no runtime que o criou; ao compartilhar,
  os testes seguintes falham com "a Tokio 1.x context ... is being shutdown". Só o container
  e a URL base ficam no `OnceCell`; cada teste abre sua própria conexão de manutenção.
- **[E4] `find_by_id` de usuário não distingue visibilidade** — a comunidade é fechada e
  qualquer membro autenticado pode ler qualquer perfil. Esconder perfis entre membros não
  compra nada e complica menções.

- **[E5] LACUNA DE ESPECIFICAÇÃO: não existe endpoint de criação de guild.** O
  `docs/api/rest-api.md` §6.3 só tem `GET`/`PATCH /guilds/{id}` e `GET /guilds`. Sem
  criação, nenhum guild existe e nada mais funciona. Em vez de inventar superfície de wire
  (C4), a criação virou subcomando de CLI: `server bootstrap --guild <nome> --owner <username>`,
  que cria o guild, o cargo `@everyone`, o canal `geral` e a associação do dono, em transação.
  Reversível e fora do contrato REST.
- **[E5] `POST /invites` exige `guild_id`** — o contrato não define o escopo de um convite
  sem guild, e `CREATE_INVITE` só é verificável contra um guild. Além disso, o cadastro
  passa a inserir a conta nova em `guild_members` do guild do convite: sem isso a conta
  nasce sem enxergar nada.
- **[E5] `GET /guilds/{id}` responde `ReadyGuild`** — é a mesma estrutura que o `READY` do
  gateway entrega (canais visíveis, categorias, cargos, membros). Um segundo tipo com os
  mesmos campos só criaria oportunidade de divergir.
- **[E5] Visibilidade de guild = ser membro não banido; visibilidade de canal = `VIEW_CHANNEL`.**
  O §5.3 pressupõe um membro e não trata o caso de não-membro. `GET /guilds/{id}` aplica
  adicionalmente a regra do contrato (ao menos um canal visível) e responde 404 quando falha.
- **[E5] Ninguém concede permissão que não tem** (`clamp_to_own`, em criação/edição de cargo
  e em overwrite de canal). O SRS não diz isso, e sem a regra `MANAGE_ROLES` equivale a
  `ADMINISTRATOR` por escalada. Exceção: quem tem `ADMINISTRATOR` concede qualquer coisa.
- **[E5] `axum::Json` e `axum::extract::Path` foram embrulhados em `crate::extract`** — o
  rejection padrão do axum devolve `422` com corpo de texto puro, e o contrato §3 não tem
  `422` nem um segundo formato de erro. Corpo com forma errada vira `400 VALIDATION_FAILED`
  nomeando o campo (extraído do caminho do `serde_path_to_error`); id malformado no path vira
  `404`, igual a um id invisível.
- **[E5] `@everyone` não pode ser apagado** — o guard está no `WHERE` do `DELETE`, então a
  linha simplesmente não casa e a rota responde 404.
- **[E5] Kick apaga a linha de `guild_members`; ban mantém a linha com `banned_at`** — assim
  o banimento sobrevive a um novo convite, e o kick não.
- **[E5] O E5 não emite `PERMISSIONS_STALE`** — o contrato §6.4 exige o dispatch, mas o
  gateway é do E6. O barramento de eventos e a ligação das rotas de E5 nele entram no E6.
  Não foi criada interface vazia para isso (CLAUDE.md §2.10).

- **[E6] `READY` é despachado antes de a sessão entrar no registro** — registrar primeiro
  deixa uma janela em que o `PRESENCE_UPDATE` de outra conexão toma a sequência 1, e o §3.1
  exige `READY` como primeiro frame da sessão. `Hub::create_session` e `Hub::attach` são
  separados exatamente por isso. Encontrado por teste, não por leitura.
- **[E6] A lacuna de resume é medida por evicção, não pelo `s` mais antigo do buffer** —
  `TYPING_START` consome sequência e nunca entra no buffer (§5), então comparar contra o
  frame mais antigo bufferizado chamaria de lacuna um evento efêmero pulado. A sessão guarda
  `evicted_through`, o maior `s` descartado por estouro; só isso torna a retomada impossível.
- **[E6] `session_id` só retoma para o próprio usuário** — o §3.3 não diz, e sem a checagem um
  `session_id` vazado entrega o histórico de eventos da sessão alheia.
- **[E6] Presença fica em memória no hub, não no banco** — `online`/`offline` derivam do
  batimento e `idle`/`dnd`/`invisible` vêm do `PATCH /users/@me/presence`. Nada disso
  sobrevive a um reinício, o que é correto: sem socket não há presença. O SRS §5.2 não tem
  tabela de presença, o que confirma a leitura.
- **[E6] Um status declarado não sobrevive à queda da conexão** — quem estava em `dnd` e caiu
  aparece `offline`, não `dnd`. Do contrário um cliente que morre deixa presença fantasma.
- **[E6] O índice de fan-out falha fechado** — se calcular os espectadores der erro, o conjunto
  volta vazio e o evento não sai. Falhar aberto viraria broadcast.
- **[E6] `GET /guilds/{id}` não emite evento** — o §5 do protocolo não define `GUILD_UPDATE`.
  `PATCH /guilds/{id}` também não emite, pelo mesmo motivo. Registrado como lacuna: o cliente
  descobre renomeação de guild na próxima leitura por REST.
- **[E6] Versão do cliente abaixo do mínimo fecha com 4010, não 4001** — 4010 dispara o fluxo
  de atualização automática (RF-36); 4001 faria o app deslogar o usuário, que não tem nada a
  ver com o problema.

- **[E7] Sintaxe de menção fixada aqui** — o contrato exige extração no servidor mas não diz
  o formato. Adotado `<@uuid>` para usuário, `<@&uuid>` para cargo e `@everyone` como palavra
  solta. A forma com colchetes existe para que prosa comum (`escreva para @joao`) não vire
  menção. Menção dentro de bloco ou span de código é inerte: colar um log não notifica ninguém.
- **[E7] O nonce usa um portão de um permissão por chave, não uma checagem simples** — dois
  envios simultâneos do mesmo nonce passariam os dois por um "já existe?". O primeiro segura o
  permit enquanto insere; o segundo bloqueia e, ao entrar, encontra a mensagem do primeiro.
  Descoberto por teste; a primeira implementação criava duas mensagens.
- **[E7] Editar é só do autor; `MANAGE_MESSAGES` apaga, não reescreve** — o contrato §6.5 diz
  "autor" em `PATCH` e "autor ou MANAGE_MESSAGES" em `DELETE`. Colocar palavras na boca de
  alguém é um poder diferente de remover.
- **[E7] `@everyone` sem `MENTION_EVERYONE` é texto, não menção** — a linha não é gravada e
  ninguém é notificado, mas o conteúdo fica intacto. Sem isso qualquer membro levanta badge em
  todo mundo.
- **[E7] Remover a própria reação exige visibilidade, não `ADD_REACTIONS`** — perder a
  permissão não pode deixar uma reação sua presa lá.
- **[E7] O marcador de leitura precisa apontar para mensagem do mesmo canal** — recontar
  menções contra um id de outro canal limparia o badge errado.
- **[E7] `mark_read` recontabiliza em vez de zerar** — uma menção que chegou entre o último
  render do cliente e a chamada precisa sobreviver, ou some sem ser lida.
- **[E7] Prévia de resposta a mensagem apagada mostra "mensagem apagada"** — o cabeçalho
  continua renderizando; sumir com ele faria a resposta perder o contexto.
- **[E7] O `HEAD` no R2 antes de persistir anexo NÃO foi feito** — o contrato §6.5 exige, e o
  cliente de armazenamento é do E8. O E7 valida RF-11a (tamanho, tipo, quantidade) e
  `ATTACH_FILES`, mas aceita qualquer `r2_key`. Item carregado para o E8.

- **[E8] Arquivo acima do limite responde 413, tipo proibido responde 400** — o contrato §3
  reserva `PAYLOAD_TOO_LARGE` para o limite do RF-11a; um tipo não permitido não é grande, é
  inválido, e sai como `VALIDATION_FAILED` nomeando `content_type`.
- **[E8] A chave é `att/{uuidv7}/{nome-saneado}`** — o prefixo único evita colisão entre dois
  `captura.webp`, e o saneamento impede que um nome vire caminho. O nome original sobrevive
  para o diálogo de download.
- **[E8] Assinatura é local, sem chamada de rede** — uma indisponibilidade do R2 não bloqueia
  o presign; ela aparece no `PUT` do cliente, onde o erro é acionável.
- **[E8] O `HEAD` antes de persistir devolve `VALIDATION_FAILED`, não 404** — o recurso que
  falta é o objeto que o cliente diz ter enviado, e o campo culpado é `attachments`.
- **[E8] A coleta de órfãos tem carência de 24 h e consulta o banco por chave** — sem a
  carência, um objeto recém-enviado seria apagado enquanto o usuário ainda escreve a mensagem.
- **[E8] Exclusão lógica de mensagem NÃO libera o objeto** — a linha de `attachments`
  permanece, então a chave continua referenciada. É deliberado: o mapeamento cruzado da ponte
  depende da linha. O objeto só vira órfão quando o canal é removido fisicamente e a cascata
  leva mensagem e anexo.
- **[E8] Erros do SDK da AWS são formatados com `DisplayErrorContext`** — o `Display` puro
  imprime só "service error", sem a cadeia de causa, o que torna o log inútil.

- **[E9] A unicidade da conversa 1:1 usa um lock consultivo sobre o par canônico** — o SRS §5.2
  diz que a unicidade "é garantida na aplicação", e checar-e-inserir não basta: sob
  `READ COMMITTED` as duas transações leem antes de qualquer uma commitar e o par termina com
  dois canais. `pg_advisory_xact_lock` sobre `min(a,b):max(a,b)` dá o ponto de serialização,
  é liberado por commit ou rollback e não custa nada fora da colisão. Descoberto por teste.
- **[E9] `POST /dms` responde 200 ao resolver e 201 ao criar** — resolver não é criar, e o
  cliente precisa distinguir para não duplicar a aba na segunda árvore de navegação.
- **[E9] Um ghost user não pode ser destinatário** — ele não tem sessão nem forma de ler a
  conversa; aceitar criaria um canal morto.
- **[E9] O criador do grupo é quem tem `added_by = user_id`** — `channels` não tem
  `created_by` no SRS §5.2, e a primeira linha de participante identifica quem abriu.
  Alternativa descartada: acrescentar coluna, o que é alteração de schema normativo.
- **[E9] Adicionar participante a um `dm` é 409, não promoção silenciosa a `group_dm`** —
  mudar o tipo do canal por baixo mudaria a resolução de unicidade do par.
- **[E9] Quem sai recebe o `DM_PARTICIPANT_REMOVE` explicitamente** — no momento do despacho
  ele já não está no conjunto de destinatários, e sem o endereçamento direto o cliente dele
  nunca fecharia a conversa.
