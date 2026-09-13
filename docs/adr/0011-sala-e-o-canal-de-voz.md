# ADR-0011 — A sala é o canal de voz do Discord

- **Status:** Aceito
- **Data:** 2026-09-12

## Contexto

Na v1 a sala do LiveKit era derivada de um canal de voz **nosso**:
`room_name(channel_id) -> "channel-<uuid>"`, implementado e testado em
`crates/api/src/voice.rs`. Com o [ADR-0010](0010-autorizacao-derivada-do-discord.md) os
canais próprios deixam de existir.

Resta a pergunta de produto: como o usuário escolhe onde compartilhar? Qualquer resposta
que envolva uma lista de salas no nosso aplicativo é uma segunda árvore de navegação, que
o usuário precisa manter mentalmente sincronizada com a do Discord.

## Decisão

**Não existe seletor de sala.** A sala é o canal de voz do Discord em que o usuário já
está.

O bot observa os estados de voz do Discord. Quando o usuário entra num canal de voz, o
backend informa o aplicativo, que entra na sala correspondente do LiveKit sem interação.
Quando sai do canal de voz, sai da sala. O nome da sala é derivado do snowflake do canal
de voz do Discord.

## Consequências

- Zero configuração e zero UI de navegação. O modelo mental é inteiramente o do Discord:
  "estou no canal `#jogos`, então quem está no `#jogos` vê minha tela".
- Quem está na sala, e portanto quem pode assistir, é resposta que o Discord já dá.
- O aplicativo pode ficar na bandeja o tempo todo e só aparecer quando há algo para ver —
  o que torna o custo em repouso um requisito de primeira ordem, não um detalhe.
- **Custo aceito:** quem não consegue entrar num canal de voz do Discord não consegue
  compartilhar. Isso é deliberado. Se a voz do Discord também estiver bloqueada na região,
  a premissa do produto muda e a decisão precisa ser revisitada — não remendada.
- A assinatura de `room_name` e as rotas de token passam a receber um snowflake do Discord
  em vez de um `Uuid` interno. É mudança mecânica sobre código que já existe e tem testes.

## Alternativas rejeitadas

- **Lista de salas no aplicativo.** Segunda árvore de navegação, sincronização manual, e
  a pergunta "em qual sala eu entro?" que o Discord já respondeu.
- **Sala por servidor, não por canal.** Colapsa canais de voz distintos numa sala só, o
  que quebra a expectativa de privacidade de um canal restrito a um cargo.
- **Link avulso de compartilhamento, sem Discord.** Recurso tentador e caminho direto para
  reconstruir autenticação, autorização e convites — exatamente o que o
  [ADR-0008](0008-complemento-ao-discord.md) tira do escopo. Fica registrado como candidato
  a v2, não como atalho.
