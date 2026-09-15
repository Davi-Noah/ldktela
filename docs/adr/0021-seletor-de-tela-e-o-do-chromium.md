# ADR-0021 — O seletor de tela é o do Chromium, e continua sendo até depois do S9

- **Status:** Aceito
- **Data:** 2026-09-14

## Contexto

O S7 pede interface própria para o compartilhamento, sem o seletor do Chromium e
sem a barra de "você está compartilhando" — que destoam do resto do aplicativo.

A API do WebView2 para isso é `CoreWebView2.ScreenCaptureStarting`, e ela expõe
exatamente quatro membros: `Cancel`, `Handled`, `OriginalSourceFrameInfo` e
`GetDeferral`. **Não há como fornecer a fonte.** Dá para recusar a captura — e
aí não existe stream — ou deixar o seletor aparecer. Não há terceira opção, e
isso não é questão de esforço: a capacidade não existe na plataforma.

O único caminho para um seletor próprio é tirar a **publicação** do WebView:
captura nativa no Windows e publicação pelo SDK Rust do LiveKit, mantendo a
visualização no WebView. É o que o Discord faz.

## Decisão

Por ora, o seletor é o do Chromium.

A migração da publicação para o Rust nativo fica **reavaliada depois do S8 e do
S9**, não descartada. O momento é esse por um motivo concreto: o S9 já vai exigir
código nativo de áudio ([ADR-0025](0025-audio-exclui-o-discord.md)), então parte
do custo de entrada — captura nativa, ponte com o WebView, empacotamento — já
estará paga quando a decisão voltar à mesa.

A supressão da barra do Chromium fica como **experimento barato** via
`additionalBrowserArgs` no `tauri.conf.json`. Se não funcionar, não funciona: não
vale reverter o [ADR-0001](0001-midia-no-webview.md) por uma barra.

## Consequências

- O S7 entrega tudo o que **não** depende disso: múltiplas telas, destacar em
  outro monitor, volume por tela, tempo de transmissão, dono de cada tela,
  qualidade. Esses são impossíveis de contornar por fora e valem o tempo; o
  seletor é um incômodo estético com contorno zero.
- O usuário vê duas estéticas no fluxo de compartilhar: a nossa e a do Chromium.
  Custo assumido e visível.
- Quando a migração nativa voltar, este ADR é substituído, e o
  [ADR-0001](0001-midia-no-webview.md) passa a valer só para a visualização.

## Alternativas rejeitadas

- **Migrar a publicação para o Rust agora.** Resolveria seletor, barra, áudio por
  aplicativo e encoder por hardware de uma vez — mas são semanas, traz
  `webrtc-sys` para o build, e deixa dois SDKs de mídia para manter. O S7 inteiro
  atrasaria por um ganho que é, hoje, estético.
- **Cancelar o evento e desenhar um seletor nosso.** Cancelar não dá acesso a
  fonte nenhuma; produz `NotAllowedError` e nada mais. Seria um seletor bonito
  que não captura.
