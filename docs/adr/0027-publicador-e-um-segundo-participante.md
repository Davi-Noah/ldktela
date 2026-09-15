# ADR-0027 — Quem publica entra na sala como um segundo participante

- **Status:** Aceito
- **Data:** 2026-09-14
- **Decorre de:** [ADR-0026](0026-publicacao-no-rust-nativo.md)

## Contexto

Com a publicação no core Rust e a visualização no WebView, uma pessoa que compartilha
passa a ter **duas** conexões com o LiveKit: a do WebView, que assiste, e a do core, que
publica.

No LiveKit a identidade é única por sala. Uma segunda conexão com a mesma identidade
**desconecta a primeira** — não é um erro que dê para tratar, é o comportamento do
servidor. Então as duas conexões precisam de identidades diferentes, e o resto do sistema
precisa saber que as duas são a mesma pessoa.

Isso importa porque a identidade é a chave de três coisas que já existem: `room_presence`,
a expulsão imediata quando o Discord revoga o acesso (RF-08) e os webhooks que produzem
`SHARE_START`/`SHARE_STOP`.

## Decisão

A conexão do WebView mantém a identidade canônica, o UUID do usuário. A conexão que
publica entra como `{uuid}~pub`.

**O sufixo é decidido pelo servidor**, ao emitir um token de publicação, e vai assinado
dentro do JWT. O cliente não escolhe a própria identidade — se escolhesse, poderia assumir
a de outra pessoa. O `~` foi escolhido por não ocorrer em UUID, o que torna o sufixo
inequívoco de separar.

O tratamento dos webhooks passa a distinguir os dois:

- `participant_joined` / `participant_left` de um `~pub` **não escrevem presença**.
  Presença significa "esta pessoa está na sala", e quem representa a pessoa é a conexão do
  WebView. Sem isso, parar de compartilhar removeria a pessoa da sala.
- `participant_left` de um `~pub` **encerra a sessão**, libera a vaga de publicador e emite
  `SHARE_STOP`. É assim que a queda do core é percebida, e não pelo tempo do token.
- `track_published` / `track_unpublished` seguem iguais: a identidade é normalizada para o
  UUID antes de qualquer coisa.

A expulsão por revogação remove **as duas** identidades. Tirar só o espectador deixaria a
tela no ar.

No cliente, o espectador ignora o próprio `~pub`: assinar a própria tela custaria egress e
ingress para receber de volta o que já está na máquina.

## Consequências

- `room_presence` não muda de forma. Foi um dos motivos de o
  [ADR-0022](0022-destacar-tela-usa-document-pip.md) ter recusado janelas Tauri extras; aqui
  o problema não aparece porque participante-sombra nunca entra em presença.
- A contagem de espectadores continua correta: ela sai de `room_presence`, onde só há
  gente.
- O `~pub` aparece na lista de participantes do LiveKit. É invisível no produto, mas
  aparece em qualquer ferramenta de inspeção do SFU. Fica registrado para não parecer bug.
- A vaga de publicador (`claim_publisher`) continua indexada pelo usuário autenticado, não
  pela identidade. Nada a mudar.

## Alternativas rejeitadas

- **Mesma identidade nas duas conexões.** O servidor desconecta a primeira. Não é
  configurável do nosso lado.
- **Uma conexão só, fazendo tudo no core.** Obrigaria a renderizar vídeo remoto em janela
  nativa e jogar fora a interface React — recusado no [ADR-0026](0026-publicacao-no-rust-nativo.md).
- **O publicador fica com a identidade canônica e o espectador com o sufixo.** A conexão
  persistente é a do espectador; a de publicação é transitória e na maioria das sessões
  nem existe. O nome canônico pertence a quem está sempre lá.
