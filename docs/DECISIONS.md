# Registro de decisões de implementação

Formato: `[Estágio] Nome curto — o que foi escolhido, e a alternativa descartada em meia linha.`

Este arquivo registra apenas o que a documentação normativa não decide, e as
divergências encontradas entre documentos normativos. Ele **não** altera a
especificação: conflito é registrado aqui e resolvido pela fonte de maior
autoridade (`docs/adr/` > `docs/SRS-v2.0-*.md` > `docs/websocket.md` >
`docs/rest-api.md` > `CLAUDE.md`).

> **Escopo estreitado em 2026-09-12** ([ADR-0007](adr/0007-governanca-de-decisoes.md)).
> Este arquivo passa a guardar **notas de implementação**: por que *esta linha* é como é.
> Decisão que restringe trabalho futuro — stack, formato de wire, modelo de autorização,
> ordem de roadmap, o que fica fora de escopo — vai para `docs/adr/`, um arquivo por
> decisão, com status explícito. Critério de separação no [`adr/README.md`](adr/README.md).
>
> As entradas de E0 a E11a abaixo são anteriores a essa divisão e ficam como estão. Várias
> delas descrevem código que a fatia S1 remove; nenhuma foi reescrita, porque o valor
> delas agora é histórico — inclusive o das que documentam becos sem saída.

## Divergências entre documentos e disco

- **[E0] ~~Layout de `docs/` diverge do CLAUDE.md §1~~ — RESOLVIDO em 2026-09-12.** O
  CLAUDE.md apontava `docs/srs/`, `docs/protocol/websocket.md` e `docs/api/rest-api.md`,
  caminhos que nunca existiram no disco. A tabela do §1 foi corrigida para os caminhos
  reais na reescrita do pivô. Divergência encerrada.
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

- **[E10] `websearch_to_tsquery`, não `plainto_tsquery`** — aceita aspas para frase exata e
  `-termo` para exclusão sem que o servidor precise inventar sintaxe, e nunca levanta erro de
  parse com entrada arbitrária, ao contrário de `to_tsquery`.
- **[E10] Buscar num canal invisível responde 404, não página vazia** — uma página vazia
  confirmaria que o canal existe. Só o escopo por `guild_id` devolve conjunto reduzido em
  silêncio, porque ali o canal nem é nomeado pelo solicitante.
- **[E10] Conjunto de canais vazio ou termo vazio faz curto-circuito antes da consulta** —
  além de inútil, emitir a consulta deixaria o tempo de resposta indicar se o termo existe
  em algum lugar.
- **[E10] LIMITAÇÃO MEDIDA do stemmer de português**: o Snowball não unifica plural de
  palavras em `-ão` (`reunião` → `reuniã`, `reuniões` → `reuniõ`) nem tolera acento ausente
  (`orçamento` → `orçament`, `orcamento` → `orcament`). Verificado no PostgreSQL 16 deste
  projeto, não suposto. É exatamente o caso que o P-02 do SRS §10.1 antecipa; o índice de
  trigrama já está preparado e comentado na migration 0004. Há teste que falha se esse
  comportamento mudar, para que a decisão possa ser revista com evidência.

- **[E11] O guard de câmera é admissão no momento de emitir o token, em memória** — o
  SRS §5.2 não tem coluna para intenção de câmera (`voice_states.streaming` é
  compartilhamento de tela), e quando a quarta câmera já está publicada o egress já foi
  gasto. O LiveKit continua sendo a autoridade sobre o que está publicado; o backend faz
  controle de admissão. Coerente com a instância única do RNF-17.
- **[E11] Renovar o token não consome uma segunda vaga de câmera** — o cliente renova
  silenciosamente antes de expirar (RNF-07); contar a renovação trancaria o usuário fora da
  própria câmera.
- **[E11] Pedir token sem câmera devolve a vaga** — quem desliga a câmera não pode continuar
  ocupando a quarta cadeira da sala.
- **[E11] Webhook não assinado responde 401, e webhook de sala alheia responde 204** — o
  primeiro é recusa; o segundo é assinado e válido, só não é nosso, e recusá-lo faria o
  LiveKit reenviar para sempre.
- **[E11] O TTL do token é limitado a 3600 s no código, não só na configuração** — o RNF-07
  fixa o teto; uma configuração acima dele é reduzida, porque token de mídia de longa duração
  é propriedade de segurança e não preferência.
- **[E11] `DATABASE_URL` e `LIVEKIT_URL` no `.env.example` passam a usar `127.0.0.1`** — no
  Windows `localhost` resolve para `::1` antes de `127.0.0.1`, e o Docker Desktop desta
  máquina não encaminha IPv6: a conexão é resetada. Nomes de variável inalterados. Custou uma
  investigação inteira; fica documentado para não custar outra.
- **[E11] Os testes reutilizam o PostgreSQL do compose quando `DATABASE_URL` existe** — cada
  binário de teste é um processo próprio, então um container por binário mantinha uma dúzia
  de PostgreSQL vivos ao mesmo tempo durante `just check`, e o esgotamento de conexões
  resultante parecia teste instável. O caminho por testcontainers continua, para CI sem
  compose. Os bancos de teste levam prefixo e id de processo no nome, e leftovers de execuções
  anteriores são varridos na inicialização.

- **[E11a] RNF-10 passa a significar sala vazia, e o fechamento e do LiveKit** — o texto do
  SRS §4.3 diz "desconexao de salas sem trafego de audio apos 15 min"; a implementacao passa a
  ser `empty_timeout` e `departure_timeout` = 900 s em `docker/livekit.dev.yaml`. Desvio
  **instruido** no follow-up do E11a, registrado aqui porque contraria a letra do SRS.
  Consequencia assumida, sem disfarce: uma sala com gente conectada e calada nao fecha mais.
  O custo de egress desse caso e proximo de zero — ninguem publicando e nada para encaminhar —
  e quem protege o orcamento de fato e o teto de 3 cameras, que continua no backend. Em troca,
  some um varredor que reimplementava, pior, um ciclo de vida que a SFU ja tem.
- **[E11a] `VOICE_IDLE_ROOM_TIMEOUT_SECONDS` sai do `.env` e do `.env.example`** — o
  `.env.example` declara que os nomes da lista sao normativos, entao remover um e desvio e
  precisa constar aqui. A variavel deixou de ter leitor: o timeout mora na configuracao do
  LiveKit. Variavel de ambiente que ninguem le e pior que variavel ausente, porque promete um
  controle que nao existe. Uma linha de comentario no `.env.example` diz para onde o ajuste foi.
- **[E11a] O LiveKit de desenvolvimento sobe com `--dev` e com webhook configurado** — sem o
  bloco `webhook` o servidor nunca chamava o backend, e todo o caminho de `VOICE_STATE_UPDATE`
  so existia sob teste. A URL usa `host.docker.internal`, com `extra_hosts` no compose para o
  mesmo arquivo funcionar em Linux. `--dev` nao substitui a chave do arquivo: so injeta um par
  proprio quando nao ha nenhum, o que foi confirmado autenticando o `livekit-cli` com a chave
  do `livekit.dev.yaml`.
- **[E11a] Os corpos de webhook viram fixtures gravadas byte a byte** — em
  `crates/api/fixtures/livekit/`, capturadas de `livekit-server` 1.8.4 dirigido por
  `livekit-cli` 2.18.4. Os testes escritos a mao usavam uma forma simplificada e nao provavam
  nada sobre o formato real (enum como nome, inteiro de 64 bits como string, chaves em
  camelCase, campos desconhecidos). A fixture de `screen_share` e derivada da real trocando um
  valor de enum, porque o `lk` publica sempre como CAMERA e nao oferece escolha de fonte.
- **[E11a] `track_source` passa a ser comparado por igualdade, nao por prefixo** — tela
  compartilhada com audio publica DUAS tracks, `screen_share` e `screen_share_audio`. Com
  `contains`, despublicar so o audio apagava o `streaming` de quem seguia com a tela na frente
  de todo mundo. Achado ao olhar o enum real.

- **[E12] Pivô de escopo: só documentação, nenhuma linha de código tocada** — o
  reposicionamento para complemento de screen share ([ADR-0008](adr/0008-complemento-ao-discord.md))
  foi registrado inteiramente em `docs/`. O código no disco continua sendo o da v1, e a
  remoção acontece na fatia S1, sob o aval pendente do
  [ADR-0016](adr/0016-poda-por-reescrita-de-migrations.md). Separar as duas coisas é
  deliberado: documento reescrito é reversível por `git revert`; poda de vinte tabelas e
  quarenta rotas, não tanto.
- **[E12] O SRS v1.2 fica no repositório, com aviso no topo, em vez de ser apagado** — ele
  guarda três coisas que a v2.0 não repete e que custaram trabalho real: o changelog das
  premissas factualmente erradas da v1.0 (§0), as armadilhas operacionais do provedor
  (§7.1) e a matriz de riscos de infraestrutura (§8). Apagá-lo jogaria fora pesquisa
  válida junto com escopo morto. Alternativa descartada: mover para `docs/archive/`, que
  quebraria os links relativos em `DECISIONS.md`.
- **[E12] `docs/rest-api.md` e `docs/websocket.md` recebem aviso de escopo em vez de
  reescrita** — reescrevê-los agora seria especificar rotas e eventos que ainda não foram
  desenhados (pareamento, sala por snowflake, revogação ao vivo), e que só ganham forma em
  S3–S5. O aviso diz o que sobrevive, o que morre em S1 e o que falta escrever, para que
  ninguém implemente contra a parte morta enquanto isso.
- **[E12] O roadmap sai do SRS e vira `docs/ROADMAP.md`** — na v1 ele era a §9 do SRS e
  ficou congelado: a fatia F1 nunca executada continuou listada como pendente por onze
  estágios, sem que nada no documento registrasse isso. Um arquivo próprio, com uma tabela
  de estado real no topo, torna o desvio visível na primeira linha em vez de na página
  quinze.

- **[S1] `bot` depende de `api`, e nao o contrario** — o CLAUDE.md §3 dizia "api e bot
  dependem de db" sem definir a relacao entre os dois. O bot e produtor de eventos que o
  `api` consome (estado de voz, mudanca de cargo, codigo de pareamento), e a replica de
  autorizacao vive no `AppState`. A seta aponta para o consumidor, o que mantem a cadeia
  linear: protocol/domain -> db -> api -> bot -> server. Alternativa descartada: um crate
  novo so para a replica, que seria abstracao antes de tres usos.
- **[S1] Os bits de permissao do Discord sao escritos a mao em `domain`, com teste de
  paridade em `bot`** — o CLAUDE.md §7 manda tirar as constantes do serenity, mas o
  serenity do workspace vem com `client` e `gateway`, que arrastam tokio para dentro de
  `domain` e violam §3. A regra sobrevive de outra forma: `crates/bot/tests/permissions_parity.rs`
  falha se qualquer um dos quatro bits divergir de `serenity::model::permissions::Permissions`.
- **[S1] `room_presence` significa "conectado a nossa sala", nao "no canal de voz"** — os
  dois diferem para quem esta na chamada sem abrir o aplicativo, e a pergunta util e quem
  consegue ver a tela. Consequencia: a tabela e escrita pelos webhooks do LiveKit, e o
  estado de voz do Discord so decide para quem mandar `ROOM_JOIN`.
- **[S1] A coluna `session_id` de `room_presence` foi removida antes de existir** — herdada
  do `voice_states` da v1, nao tinha leitor. Migration reescrita no lugar de uma migration
  aditiva, o que so e possivel porque nada foi implantado (ADR-0016).
- **[S1] O indice de fan-out do gateway foi apagado, nao adaptado** — ele existia porque
  calcular os espectadores de um canal exigia resolver permissao para cada membro do guild.
  Agora o conjunto de destinatarios e exatamente as linhas de `room_presence` daquele canal,
  que e uma consulta indexada. Um cache aqui so seria uma forma de estar errado.
- **[S1] `is_replayable` sumiu do `DispatchEvent`** — sem `TYPING_START` nao ha evento
  efemero, entao todo evento entra no buffer de retomada. A medicao de lacuna por eviccao
  (`evicted_through`) continua, porque ela nunca foi sobre o evento efemero e sim sobre
  estouro de buffer.
- **[S1] `SUM(egress_bytes)` leva `::BIGINT`** — `SUM` sobre `BIGINT` devolve `NUMERIC` no
  Postgres, e sem o cast o SQLx exige a feature `bigdecimal` no workspace inteiro por causa
  de uma query.
- **[S1] `ReplicaStale` responde 503, nao 409** — nao ha conflito de estado; o servico e que
  nao pode responder com seguranca. 503 diz ao cliente para tentar de novo, que e a acao
  correta.
- **[S1] O teste do token do LiveKit passou a inspecionar o JSON do payload** — a primeira
  versao afirmava `!payload.contains("roomAdmin")` e falhou: o LiveKit serializa a
  capacidade como `"roomAdmin":false`. Verificar ausencia de substring onde o correto e
  verificar o valor e como um teste de seguranca passa a proteger nada.
- **[S1] O bot que nao conecta nao derruba o servidor** — verificado em execucao: com token
  invalido, o Discord fecha com 4004, o bot para, e a API continua servindo e falhando
  fechada em admissao nova. Derrubar o processo levaria junto as sessoes em curso.
- **[S1] `#[ts(optional)]` sem `skip_serializing_if` e um contrato mentiroso** — `Ready.room`
  tinha so o primeiro: o tipo gerado prometia um campo ausente e o serde emitia
  `"room": null`. O cliente testava `=== undefined`, recebia `null` e quebrava com
  `TypeError` em todo `READY` sem sala — o caso comum, porque o aplicativo passa o dia na
  bandeja. Corrigido no `protocol`, nao no cliente: o tipo gerado ja estava certo, e
  `just types` regenerou sem alterar um arquivo sequer. Os dois atributos andam juntos.
- **[S2] O WebView do Linux nao faz WebRTC sem ser religado** — o WebKitGTK entrega
  `enable-webrtc` e `enable-media-stream` desligados e o Tauri nao os altera, entao o
  livekit-client recusava com "LiveKit doesn't seem to be supported on this browser" antes
  de abrir a sinalizacao: nada chegava ao SFU e nada aparecia no log do servidor. Religado
  em `enable_linux_webrtc`. Isto existe para destravar a medicao da Fase 2 com a segunda
  maquina disponivel; **nao** torna Linux plataforma suportada, o que mexeria no RNF-10 e
  exige ADR proprio.
- **[S2] O backend do `keyring` e por plataforma** — com `features = ["windows-native"]`
  sozinho, em Linux o keyring cai no store mock, em memoria, e o refresh token some a cada
  execucao. O sintoma era o aplicativo pedir pareamento em todo arranque no notebook.
- **[S9] O blob de ativação do WASAPI precisa vir de `CoTaskMemAlloc`, não da pilha** — o
  `mmdevapi` limpa o `PROPVARIANT` que recebe em `ActivateAudioInterfaceAsync`, e limpar um
  `VT_BLOB` é `CoTaskMemFree(pBlobData)`. Com o blob na pilha — que é o que a amostra
  ApplicationLoopback da própria Microsoft faz — a captura funciona perfeitamente, entrega
  os quadros certos, e destrói o heap do processo: a morte chega depois, com
  `STATUS_HEAP_CORRUPTION`, em qualquer alocação, longe dali. Levou uma sessão inteira para
  ser encontrado, e só apareceu porque existe um teste que exercita a captura de verdade
  (`cargo test -- --ignored`, em `desktop/src-tauri`). Nenhum teste de unidade o pegaria.
- **[S9] Process loopback só aceita o modo dirigido por evento** — `Initialize` com
  `LOOPBACK | EVENTCALLBACK` e `SetEventHandle` depois. O caminho de sistema inteiro é o
  contrário: num endpoint de saída em loopback o evento não dispara enquanto a máquina está
  muda, então lá a espera é por tempo. Medido: 143.520 amostras por canal em 3 s contra
  144.000 teóricas.
- **[S7] `scalability_mode` no `TrackPublishOptions` faz a publicação sair pelo ralo** — com
  VP9, definir `scalability_mode: Some("L3T3_KEY")` produz uma sessão que parece perfeita e
  não transmite nada: a captura entrega quadros, o encoder **aceita** 414 deles, a track é
  publicada, o webhook dispara, o espectador assina — e recebe **zero**. Medido contra o SFU
  de desenvolvimento, com e sem `simulcast`; o modo explícito quebra nos dois. A combinação
  que funciona é `simulcast: true` com `scalability_mode: None`, que é a mesma que o cliente
  JS usava e contra a qual o `RESULTS.md` foi medido. Fica o teste
  `publisher::tests::a_real_screen_reaches_the_sfu`, que publica uma tela de verdade e conta
  quadros **no espectador** — a única medida que distingue "transmitindo" de "parece que
  está transmitindo".
- **[S7] O adaptador do libwebrtc recusa todo quadro enquanto ninguém assina a track** — é o
  comportamento normal de uma publicação pausada (`dynacast`), e é indistinguível de uma
  captura morta se só se contar quadro aceito. Por isso a captura conta os dois: `produced`
  (convertidos e oferecidos) e `delivered` (aceitos pelo encoder).
- **[S7] Chave pública do atualizador embutida em `tauri.conf.json`, sem esconder** — o
  `pubkey` gerado por `tauri signer generate` não é segredo: o atualizador o usa para
  **recusar** um pacote sem a assinatura correspondente, então publicá-lo é o ponto — quem
  precisa ficar fora do repositório é a chave privada e a senha, guardadas como segredos do
  GitHub (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) e usadas só em
  `release.yml`.
- **[S7] A checagem de atualização é silenciosa quando falha, e nunca bloqueia** — sem rede,
  ou com o `latest.json` fora do ar, o log registra e a tentativa seguinte cobre o caso; um
  aviso na tela por uma falha que o usuário não pode resolver seria ruído. Primeira checagem
  10 s após o arranque (não compete com o boot), depois a cada 6 h — o aplicativo passa dias
  na bandeja (RNF-03) e uma versão publicada no meio disso precisa ser oferecida sem exigir
  reinício manual para notar.
- **[S7] Atualização é barra fina com botão, nunca instalação silenciosa** — o critério de
  aceite do S7 diz "automática", mas instalar e reiniciar sem perguntar interromperia uma
  transmissão em andamento sem aviso nenhum. A barra fica no rodapé, do tamanho de uma linha,
  e "Agora não" a esconde até a próxima checagem — nunca modal, porque a tela da sala existe
  para sair da frente do vídeo (`CLAUDE.md` §8).

### Revisão de interface (2026-09-16)

- **[UI] `border-line` era uma classe morta, usada em seis lugares** — `tokens.css` definia
  `--color-border`, nunca `--color-line`, e o Tailwind 4 gera utilitário a partir do token: sem
  token, a classe não existe e nenhuma borda é desenhada. O efeito era invisível na leitura do
  código e enorme na tela: ladrilhos sem contorno sobre o fundo escuro, rodapé de ladrilho sem
  divisória, e as pílulas de preset não selecionadas viradas em texto solto — que era metade da
  queixa de "visual seco". Uma linha em `tokens.css` consertou os seis.
- **[UI] `--color-text-faint` reprovava em contraste** — `#6b7482` sobre `surface-1` dá 3,49:1,
  contra os 4,5:1 que o WCAG AA pede para texto normal, e o token carrega instrução (rótulo de
  estatística, título de grupo do seletor, o aviso de validade do código de pareamento), não
  decoração. Passou a `#858e9c`, ~4,6:1. `--color-text-muted` já passava, com 6,47:1.
- **[UI] O cromo da sala ficava atrás do ladrilho em foco** — o ladrilho focado usa `z-10` e
  nenhum ancestral dele criava contexto de empilhamento, então ele competia na raiz; z-index
  positivo pinta acima de `z-auto` independentemente da ordem no DOM. Com o vídeo em foco cobrindo
  a borda inferior, "Parar de compartilhar" e "Tela cheia" ficavam inalcançáveis. O cromo virou
  uma camada única em `z-30`, e a camada precisa ser `pointer-events-none` — ela cobre a área
  inteira do vídeo, e sem isso nenhum clique chegaria ao ladrilho.
- **[UI] Erro de compartilhamento era invisível com vídeo na tela** — `error` só era renderizado
  dentro de `RoomBody`, que só monta quando **não** há vídeo. Falhar ao compartilhar enquanto se
  assistia a tela de alguém produzia um erro que ninguém via. O campo saiu do store de mídia e
  virou a camada de avisos (`store/ui.ts`), que é global e não depende de qual tela está montada.
- **[UI] O cromo sumia debaixo do ponteiro** — o temporizador só era rearmado em `pointermove`,
  então mirar num botão e parar de mexer o mouse escondia a barra sob o cursor, e um menu aberto
  ficava órfão sobre o vídeo. Agora há um contador de travamentos (`chromeHolds`): ponteiro sobre
  os controles e menu aberto são motivos que se sobrepõem, e por isso é contador e não booleano.
- **[UI] Transição de opacidade é permitida apesar do §8** — a regra proíbe "animação que rode
  enquanto há vídeo na tela". 120 ms de `opacity`, disparados por gesto e que terminam, são a
  alternativa ao pisca-pisca do `hidden`; só `opacity`, que o compositor resolve sem relayout e
  sem repintar o quadro. Respeita `prefers-reduced-motion`.
- **[UI] Esconder cromo é `opacity` mais `inert`, nunca só `opacity`** — com transparência
  sozinha, quem navega por teclado tabularia para dentro de uma barra invisível. `inert` é
  aplicado por `ref` em vez de atributo JSX, o que dispensa depender da tipagem de `inert` no
  React.
- **[UI] Colunas da grade por container query, não por `ResizeObserver`** — a contagem depende da
  forma da janela, e observar tamanho perto do caminho de render do vídeo é o que o CLAUDE.md §7
  manda evitar. `.screen-stage` declara `container-type: size` e as regras leem `data-count`; duas
  telas só ficam lado a lado acima de 1:1, porque numa janela alta e estreita empilhar dá mais
  pixel a cada uma.
- **[UI] Clique simples foca, duplo vai a tela cheia, com 220 ms de espera** — são os dois gestos
  do Discord, e o par de cliques do duplo dispararia o foco no caminho. O temporizador por
  ladrilho resolve; é o mesmo custo de um `setTimeout`, nunca por quadro.
- **[UI] Estado de UI em memória × preferência do usuário** — `store/ui.ts` guarda avisos,
  travamento do cromo e "ver a própria tela" em memória, conforme o §2.8. Uma preferência como a
  última **deveria** sobreviver ao fechamento, mas `localStorage` está proibido e não existe cofre
  de preferências no core. Fica em memória, ligada por padrão, até que exista.
- **[S7] Miniatura do seletor sai do `DesktopCapturer`, uma por vez** — capturar quinze janelas
  custa perto de um segundo, e o seletor precisa abrir na hora; a lista aparece imediata e as
  figuras entram conforme chegam. O primeiro `capture_frame` costuma voltar vazio porque o DXGI
  ainda está acordando, daí a insistência com teto de doze tentativas — sem o teto, uma fonte que
  nunca entrega quadro prenderia a thread.
- **[S7] `jpeg-encoder` em vez de RGBA cru pelo IPC** — 480×270 em RGBA são 518 KB por quadro,
  contra ~20 KB em JPEG q70. A 12 fps, é a diferença entre 6 MB/s e 240 KB/s de tráfego de IPC
  para uma imagem que vai ser desenhada e descartada. O base64 é próprio, vinte linhas, para não
  trazer mais uma dependência.
- **[S7] Preview entregue como data URL para uma `<img>`, não como bytes para um canvas** — o
  WebView decodifica cada quadro fora da thread principal e desenha sozinho; trocar `img.src` é a
  operação inteira, sem `createImageBitmap`, sem canvas e sem uma linha de JavaScript por quadro.
- **[S7] O atalho global é registrado no Rust** — `tauri-plugin-global-shortcut` do lado do core
  dispensa entrada em `capabilities/` e no `package.json`. Um atalho já tomado por outro programa
  não impede o aplicativo de subir: perde-se o atalho, e o botão e a bandeja continuam parando a
  transmissão.
- **[S7] ~~PiP nativo é a segunda janela flutuante, e não contraria o ADR-0022~~** — Document PiP e
  `requestPictureInPicture` são APIs distintas, com janelas distintas, sobre a mesma conexão do
  LiveKit: o elemento é o mesmo, o decodificador é o mesmo, e não há assinatura nova. O limite de
  **uma** janela destacada do ADR-0022 continua valendo para o Document PiP, e a troca agora é
  anunciada em vez de fechar a anterior em silêncio.

  > **Revogado em 2026-09-17 pelo [ADR-0033](adr/0033-o-pip-do-webview-sai.md).** O raciocínio
  > estava certo sobre as APIs e errado sobre o produto: a janela do PiP nativo é do Edge, com
  > controles do Edge, e um deles abre `edge://settings` — que num WebView2 termina em
  > `ERR_INVALID_URL` dentro do nosso aplicativo. "Segunda janela" também pressupunha que a
  > primeira abria, e ela não abria.

- **[Lançamento] O endereço do servidor é de build, e a build para sem ele** — `VITE_SERVER_ORIGIN`
  vivia só num `.env.local` fora do git, então o `.msi` que o CI publicava apontava para
  `http://127.0.0.1:8080`: a máquina de quem instalasse. Passou para `desktop/.env.production`,
  versionado, e o `vite.config.ts` recusa empacotar sem ele — e recusa também quando o endereço
  não está no `connect-src` do CSP, que é a mesma falha pelo outro lado (o aplicativo abre, a
  interface pinta, e cada requisição é bloqueada sem erro nenhum). Não há variável de repositório
  como escapatória de propósito: uma variável não definida chega como string vazia e sobrepõe o
  arquivo, e trocar o endereço sem trocar o CSP é exatamente o estado que a verificação existe
  para impedir.

- **[Lançamento] A chave do LiveKit saiu do arquivo versionado** — `docker/livekit.remote.yaml`
  carregava o segredo com que o servidor implantado estava rodando. Agora vem de `LIVEKIT_KEYS`,
  montado pelo compose a partir do `.env.remote`, com `${...:?}` para a pilha parar em vez de
  subir sem chave. Rotacionar a chave da VM é passo de operação, em `docs/deploy-oracle.md`.

- **[Lançamento] Uma porta UDP multiplexada, não uma faixa de vinte** — cada participante gasta
  uma porta (o core Rust) ou duas (o WebView), então quem compartilha custa três, e vinte portas
  acabam num canal de voz de sete pessoas. O modo de falha é mudo: o ICE não conecta, a
  sinalização continua de pé e simplesmente não aparece vídeo. `rtc.udp_port: 7882`, conferido
  com `livekit-server ports`. Exige abrir a porta na VM antes de subir.

- **[Diagnóstico] A conexão de mídia que desistiu passou a ocupar espaço na tela** — `failed`
  produzia uma sala de aparência normal (nome do canal, lista de gente, botão de compartilhar)
  sobre uma conexão que não existia, e a única pista era uma torrada já desaparecida. O motivo
  agora fica no corpo da sala, e o botão de compartilhar sai junto: sem conexão de espectador,
  transmitir dali manda a tela para uma sala que este aplicativo não está vendo. O caso comum é
  o mesmo usuário instalado em duas máquinas — inclusive PC e Windows Sandbox, que é como o
  defeito foi encontrado.

- **[Diagnóstico] Zero quadro capturado ganhou aviso próprio** — o painel sabia dizer "capturada
  mas não codificada", que é transmissão pausada por falta de espectador, e não sabia dizer
  "nada saiu da tela". São coisas diferentes: a segunda acontece onde não há GPU acessível
  (máquina virtual, Windows Sandbox, sessão remota), o compartilhamento sobe e publica
  normalmente, e todo mundo vê tela preta sem um número no painel que explique.

- **[P-01] `ROOM_MAX_PUBLISHERS` sobe de 2 para 10** — o teto nunca foi limite técnico do LiveKit
  nem do WebRTC, sempre foi um contador em memória em `crates/api/src/livekit.rs::claim_publisher`,
  comparado contra a variável de ambiente. A SRS já registrava isso como decisão em aberto ("P-01,
  revisar quando o uso mostrar demanda"), e a demanda apareceu: o dono do produto quer usar com o
  servidor inteiro (até 10 pessoas). Custo aceito conscientemente — a cada publicador simultâneo o
  egress multiplica por espectador (RF-32): no pior caso, 10 publicadores e alguns espectadores por
  hora de uso diário ainda cabem no teto de 10 TB/mês da Oracle Free Tier com folga (ver a tabela em
  `docs/deploy-oracle.md`), mas o valor deixa de ser folgado por padrão — vale medir egress real
  (RNF-05) se o uso crescer. Atualizado em `.env.example`, `docker/env.remote.example`,
  `docs/DESTRAVAR.md` e no `.env.remote` da VM; a SRS (P-01) reflete o novo padrão.

- **[#1] A réplica guarda quem está em call, e o READY consulta isso primeiro** — antes, o único
  gatilho para o cliente entrar numa sala era uma *transição* de voz; quem já estava na call ao
  abrir o app recebia um READY vazio (a presença só existe depois de entrar na sala do LiveKit) e
  precisava sair e voltar. Agora o `GUILD_CREATE` traz o retrato de `voice_states`, cada
  `VOICE_STATE_UPDATE` o mantém, e o READY usa o canal de voz — com a mesma checagem de
  `can_join_room` da transição — antes de cair na presença.
