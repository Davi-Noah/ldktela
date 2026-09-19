# ADR-0034 — Destacar tela é um popup do mesmo documento, não Document PiP

- **Status:** Aceito
- **Data:** 2026-09-19
- **Substitui o mecanismo do** [ADR-0022](0022-destacar-tela-usa-document-pip.md) **e o gate do**
  [ADR-0033](0033-o-pip-do-webview-sai.md)

## Contexto

O ADR-0033 escondeu o botão de destacar onde `documentPictureInPicture` não existisse. O botão
continuou aparecendo e continuou falhando (issue #3), porque a API **existe** no WebView2.

Medido dentro do WebView2 do próprio aplicativo, pelo protocolo do DevTools, com gesto de
usuário simulado para que ativação não fosse a desculpa:

```
navigator.userAgent            ... Chrome/153 ... Edg/153
'documentPictureInPicture' in window   true
isSecureContext                 true
requestWindow(...)              InvalidStateError: Internal error: no window
```

A API existe e o hospedeiro não cria a janela dela. Não é flag — o
`--enable-features=DocumentPictureInPictureAPI` não muda nada — e não há evento do WebView2 que
permita ao aplicativo fornecer essa janela. No WebView2, Document PiP é inalcançável.

A mesma medição mostrou por que a alternativa óbvia também falhava: `window.open('about:blank')`
retornava `null`. O Tauri nega janelas novas por padrão.

## Decisão

**Destacar abre um popup `about:blank` pelo próprio documento e move o `<video>` para ele.** A
ideia do ADR-0022 fica inteira — o elemento é movido, não recriado; mesma trilha, mesma conexão,
mesmo decodificador, nenhuma assinatura nova —, só troca a janela que o recebe.

**A janela principal passa a ser criada em Rust** (`create: false` no `tauri.conf.json`,
`build_main_window` em `lib.rs`), para poder registrar `on_new_window`. A regra é estreita:
`about:blank` é permitido, **qualquer outro endereço continua negado**, como antes.

**A janela entregue é nossa, não o popup padrão do WebView2.** Permitir com `Allow` funcionava,
mas o popup padrão vem com uma barra de endereço mostrando `about:blank` e o globo do navegador
— interface de navegador dentro do produto, o mesmo defeito que tirou o PiP nativo no
ADR-0033. `on_new_window` devolve `Create` com uma `WebviewWindow` construída por
`window_features`, que herda posição, tamanho e o ambiente do WebView2 de quem abriu (sem o
mesmo ambiente o WebView2 recusa). Resultado verificado: ícone do aplicativo, título
`ldktela`, barra escura, nenhuma interface de navegador.

**Só a janela principal esconde ao fechar.** O `on_window_event` que manda a principal para a
bandeja valia para toda janela; na destacada, esconder em vez de fechar deixaria o vídeo preso
numa janela invisível, porque o documento que a abriu nunca saberia que ela saiu.

O flag `DocumentPictureInPictureAPI` e o gate de capacidade saem: não havia o que habilitar, e o
gate testava a pergunta errada.

## Consequências

**Fica fácil:** destacar funciona no WebView2, sem segunda conexão com o LiveKit.

**Fica diferente:** o popup não é "sempre no topo" como uma janela de PiP seria. É uma janela
comum do sistema, que se arrasta para outro monitor — que é o caso de uso do RF-33.

**Fica proibido:** afrouxar o `on_new_window`. Permitir outros endereços transformaria qualquer
link numa janela do WebView2 com a sessão do aplicativo.

## Alternativas rejeitadas

**Segunda janela do Tauri.** Contexto de JavaScript separado: o `<video>` não atravessa, e
reproduzir a tela lá exigiria uma segunda conexão com identidade própria — tocando presença,
contagem de espectadores e a revogação contínua do ADR-0010.

**Continuar escondendo o botão.** Deixaria o RF-33 sem implementação quando existe uma que
funciona.
