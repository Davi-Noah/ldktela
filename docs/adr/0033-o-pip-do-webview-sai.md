# ADR-0033 — O Picture-in-Picture do WebView sai; destacar é tudo ou nada

- **Status:** Aceito
- **Data:** 2026-09-17
- **Revoga a nota "[S7] PiP nativo é a segunda janela flutuante"** de `docs/DECISIONS.md`
- **Complementa o** [ADR-0022](0022-destacar-tela-usa-document-pip.md)

## Contexto

O produto tinha dois botões para a mesma ideia — tirar uma tela da grade e pô-la numa janela
solta — e nenhum dos dois funcionava direito.

**"Janela flutuante"** chamava `video.requestPictureInPicture()`, o Picture-in-Picture nativo
do navegador. Ele abre, e o que abre é **uma janela do Edge**: moldura do Edge, botão "Voltar
para a guia" do Edge, um controle de som do Edge e uma engrenagem do Edge. A engrenagem aponta
para `edge://settings/appearance/browserBehavior` — que num WebView2 não existe, e termina numa
página de erro `ERR_INVALID_URL` dentro da janela do nosso aplicativo.

Não há CSS que conserte isso. É interface do navegador, desenhada para um navegador, e o
WebView2 é o mesmo motor sem as páginas que ela pressupõe. O aplicativo não consegue nem
esconder os botões nem fazê-los funcionar.

**"Destacar em outra janela"** usa o Document Picture-in-Picture, cuja janela é nossa por
inteiro (ADR-0022). Só que neste WebView2 ela responde sempre `Não consegui abrir a janela
destacada nesta versão do Windows`. O runtime é Chromium 153, muito além do 116 em que a API
estreou, então o código existe no binário — o que não se sabe é se o WebView2 a expõe.

O resultado combinado é o pior arranjo possível: **o recurso que a gente controla não abre, e o
que abre a gente não controla.**

## Decisão

**O Picture-in-Picture nativo sai do produto.** `video.disablePictureInPicture = true`, e o
botão deixa de existir. Destacar uma tela passa a ter um caminho só, o do ADR-0022.

**O botão de destacar só aparece onde a API existe.** `pictureInPictureSupported()` é lido uma
vez e decide se o botão é desenhado. Um botão que responde sempre "não consegui" é pior do que
botão nenhum: ele ensina que o aplicativo está quebrado, quando o que falta é um recurso do
sistema.

**A janela pede o recurso ao WebView2 explicitamente**, por `additionalBrowserArgs`:

```
--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --enable-features=DocumentPictureInPictureAPI
```

O primeiro trecho é o padrão do wry e precisa ser repetido — definir `additionalBrowserArgs`
**substitui** o padrão, não acrescenta a ele.

> **Isto é um experimento com resultado visível.** Se o WebView2 passar a expor a API, o botão
> de destacar aparece; se não, ele não aparece. A presença do botão *é* a medição, e ela custa
> um olhar em vez de uma sessão de depuração.

## Consequências

**Fica fácil:** não há mais dois botões para a mesma coisa, nem interface de navegador
aparecendo dentro do produto, nem engrenagem que abre página de erro.

**Fica caro:** onde o Document PiP não existir, **não há como destacar tela**. Sobram o foco
(clique) e a tela cheia (duplo clique, ou `F`), que resolvem "quero ver esta tela grande" mas
não resolvem "quero esta tela sobre outro aplicativo".

**Fica proibido:** reintroduzir `requestPictureInPicture` como alternativa. A janela não é
nossa, e os botões dela continuarão apontando para páginas que um WebView2 não tem.

## Alternativas rejeitadas

**Manter os dois e só consertar a engrenagem do Edge.** Não é nossa para consertar: é a
interface do navegador, servida pelo processo do WebView2.

**Manter só o PiP nativo, já que é o que abre.** Seria escolher o caminho que exibe controles
quebrados do Edge por cima do vídeo — e o som ali é o do elemento, não o do participante, então
até o controle de volume mentiria em relação ao do ladrilho (RF-35).

**Uma segunda janela do Tauri.** Já rejeitada no ADR-0022 e continua rejeitada: contextos de
JavaScript separados, o `<video>` não atravessa, e reproduzir a tela lá exigiria uma segunda
conexão com o LiveKit, com identidade própria — o que muda `room_presence`, a contagem de
espectadores e o egress.
