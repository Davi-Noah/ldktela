# ADR-0037 — O bot não anuncia a transmissão no canal

- **Status:** Aceito
- **Data:** 2026-09-21
- **Substitui parcialmente o** [ADR-0024](0024-tag-live-no-apelido.md), que tratava o anúncio
  como o sinal principal

## Contexto

Ao começar uma sessão, o bot publicava **uma** mensagem no canal de texto (RF-23) e a editava
enquanto ela durasse. O texto mencionava quem transmite: `🔴 <@id> está compartilhando a tela em
**canal** · N assistindo`.

A menção é o problema. Ela **notifica** a pessoa que acabou de clicar em compartilhar — sobre a
própria ação —, toda vez que alguém transmite. Em uso real, a comunidade recebia essa notificação
a cada transmissão, de si mesma.

Não é um defeito de texto. Enquanto houver uma mensagem que nomeia quem transmite, o Discord a
tratará como uma menção. Tirar só o `<@id>` deixaria uma mensagem dizendo "alguém está
compartilhando" em todo canal, para um público que já está na chamada de voz e já vê quem
transmite.

## Decisão

**O bot não posta, não edita e não apaga nenhuma mensagem de canal.** Sai a mensagem inteira:
a publicação, a edição de contagem e o "A transmissão terminou".

**A tag `[🔴LIVE]` no apelido continua** (RF-38 a RF-40). Ela não notifica ninguém, e é o único
sinal de Discord que sobra.

O bot deixa de precisar da permissão de enviar mensagens. A resposta do `/tela` é uma resposta de
interação, efêmera, que não depende dela.

## Consequências

- Dono do servidor e quem tem cargo acima do bot **continuam sem sinal nenhum no Discord**: o
  Discord não deixa o bot renomeá-los, e o ADR-0024 já registrava isso — dizendo, porém, que o
  anúncio cobria esse caso. Com o anúncio fora, o que cobre é o próprio aplicativo: quem está no
  ldktela vê a lista da sala e a etiqueta AO VIVO.
- Quem está no canal de voz mas **não** tem o aplicativo aberto deixa de saber que há uma
  transmissão, exceto pela tag. Isso é aceito: sem o aplicativo, a pessoa não teria como assistir
  de qualquer jeito.
- O link profundo (RF-24) perde o lugar onde vivia. Ele já estava adiado
  ([ADR-0029](0029-link-profundo-espera-uma-pagina-https.md)), e só volta se houver uma nova
  superfície para carregá-lo.
- A coalescência de 2 s continua: ela agora protege as edições de apelido contra o limite de
  taxa, que é o mesmo problema com um objeto diferente.
- O canal de texto associado (P-05) deixa de ser uma pergunta em aberto.

## Alternativas rejeitadas

- **Manter a mensagem sem a menção.** Preserva o sinal para o dono do servidor, mas deixa uma
  mensagem por sessão em todo canal, dizendo o que quem está na chamada já vê.
- **Mencionar sem notificar** (`allowed_mentions` vazio). A menção aparece como nome destacado e
  não toca o celular. É a alternativa mais barata, e foi descartada por decisão do dono do
  projeto: o pedido foi remover a mensagem, não suavizá-la.
- **Tornar opcional por servidor.** Exigiria uma configuração por servidor, e o projeto não tem
  onde guardá-la; seria abstração para um caso sem segundo uso.
