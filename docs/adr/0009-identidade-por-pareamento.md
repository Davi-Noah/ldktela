# ADR-0009 — Identidade por pareamento via bot do Discord

- **Status:** Aceito
- **Data:** 2026-09-12

## Contexto

A v1 implementou autenticação própria completa (estágio E4, em produção no repositório):
cadastro por convite, e-mail e senha com Argon2id, JWT de 15 min e refresh token opaco
rotativo com detecção de reúso por família.

Com o pivô do [ADR-0008](0008-complemento-ao-discord.md), todo usuário do produto é, por
definição, membro de um servidor do Discord — ele **já tem** identidade. Pedir que crie
uma segunda conta é atrito puro: uma senha a mais para esquecer, um fluxo de convite a
mais para administrar, e nenhuma informação que já não tenhamos de graça.

## Decisão

A identidade vem do Discord, por **pareamento com código efêmero emitido pelo bot**.

Fluxo: o usuário roda um comando de barra no Discord (`/tela parear`); o bot responde com
uma mensagem efêmera contendo um código curto, válido por poucos minutos e de uso único;
o usuário digita o código no aplicativo; o backend resolve o código para o
`discord_user_id` de quem o pediu e emite o par de tokens já existente.

A máquina de tokens da v1 (JWT curto + refresh opaco rotativo com detecção de reúso de
família) é **mantida integralmente**. Ela é boa, está testada, e o pareamento apenas
substitui o que acontece antes dela.

## Consequências

- Somem: `argon2`, a tabela `invites` e seu repositório, as colunas `email` e
  `password_hash`, as rotas `POST /auth/register` e `POST /auth/login`, e o rate limit de
  login. Entra uma tabela pequena de códigos de pareamento pendentes.
- A tabela `users` passa a ser um cache do perfil do Discord: `discord_user_id`,
  nome de exibição e URL de avatar, atualizados no pareamento e por evento do gateway.
- O pareamento **é** o controle de acesso ao produto: só recebe código quem consegue
  executar o comando num servidor onde o bot está instalado. A tabela `invites` some sem
  substituto porque o Discord já é o portão.
- O refresh token continua no cofre do sistema operacional via core Rust, nunca em
  `localStorage` — regra inalterada.
- O código de pareamento é credencial de curta duração: uso único, expiração de 5 minutos,
  comparação em tempo constante e limite de tentativas por usuário.

## Alternativas rejeitadas

- **OAuth2 do Discord (authorization code + PKCE).** É o caminho convencional e foi
  rejeitado pela razão que define este produto: exige que **o cliente** alcance
  `discord.com` para completar o fluxo, e o produto existe justamente para regiões onde o
  acesso ao Discord é restrito ou degradado. O pareamento por código atravessa o cliente
  do Discord que o usuário já tem aberto e funcionando, e o backend — que roda em outra
  jurisdição — é quem fala com a API do Discord. Secundariamente, OAuth em aplicativo
  desktop ainda exige servidor de loopback ou registro de esquema de URL, que é mais
  superfície nativa do que temos hoje.
- **Device authorization grant (RFC 8628).** Seria o encaixe perfeito, mas o Discord não
  oferece esse tipo de concessão.
- **Manter e-mail e senha.** Segundo sistema de contas, com custo de suporte real
  (recuperação de senha, verificação de e-mail) e zero benefício sobre uma identidade que
  já existe e já é a que a comunidade usa.
