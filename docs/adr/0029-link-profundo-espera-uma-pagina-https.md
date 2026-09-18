# ADR-0029 — O link profundo espera uma página https; o esquema `ldktela://` não basta

- **Status:** Aceito
- **Data:** 2026-09-16

## Contexto

O critério de aceite do S8 pede que o anúncio no Discord traga um "link profundo
que abre o aplicativo direto na sala, e se não houver aplicativo, a página de
download".

O caminho óbvio é registrar o esquema `ldktela://` e pôr `ldktela://room/<id>` na
mensagem. Foi implementado e depois revertido, por dois motivos que só aparecem
ao testar.

**O Discord não transforma esquema próprio em link clicável.** Só `http` e
`https` viram link — em texto puro, em link mascarado `[texto](url)` e em botão
de componente. `ldktela://room/123` na mensagem é texto morto que o usuário teria
de copiar e colar em algum lugar. Entregar isso seria entregar nada e chamar de
recurso.

**E o link não teria muito o que fazer.** A sala é o canal de voz do Discord
([ADR-0011](0011-sala-e-o-canal-de-voz.md)): não existe seletor de sala, e o
aplicativo segue o canal sozinho. "Abrir direto na sala" é, na prática, trazer a
janela para a frente — coisa que o ícone da bandeja já faz.

## Decisão

Por ora o anúncio não traz link. Diz quem está transmitindo, em qual canal, e
quantos assistem — que é o sinal que interessa e funciona para todo mundo.

O esquema `ldktela://` **não fica registrado**: um plugin, uma escrita no
registro do Windows e um caminho de código que nada exercita, só para um link
que ninguém consegue clicar.

O que destrava, quando alguém quiser: **uma página https de redirecionamento**.
O repositório já existe em `github.com/gbrlevi/ldktela`, então o GitHub Pages
serve. A página em `/room/<id>` tenta `ldktela://room/<id>` e, se nada abrir,
mostra o download. Aí sim o link entra na mensagem, porque aí sim ele é clicável
e tem o que fazer nos dois casos — com o aplicativo e sem ele.

## Consequências

- O S8 entrega o anúncio e a tag `[LIVE]`, e fica devendo o link. A dívida está
  escrita aqui em vez de virar uma linha verde no roadmap que não corresponde ao
  produto.
- Quando a página existir, o trabalho é: publicar o redirecionador, voltar o
  `tauri-plugin-deep-link` (a configuração é o esquema em `tauri.conf.json` e um
  `on_open_url` que revela a janela) e acrescentar a URL em `describe()`, no
  `crates/bot/src/announce.rs`.
- A página é também o único lugar onde "se não houver aplicativo, a página de
  download" pode acontecer. Sem ela, esse metade do requisito não tem onde
  morar.

## Alternativas rejeitadas

- **Pôr `ldktela://room/<id>` como texto na mensagem.** Não é clicável; vira uma
  instrução de copiar e colar no meio de um anúncio.
- **Registrar o esquema agora e usar depois.** Dependência e escrita no registro
  do Windows sem nada exercitando — exatamente o que o `CLAUDE.md` §2.11 e §2.12
  mandam não carregar. O custo de voltar é pequeno e está escrito acima.
- **Botão de componente do Discord.** Botão de link também só aceita `http(s)`.
