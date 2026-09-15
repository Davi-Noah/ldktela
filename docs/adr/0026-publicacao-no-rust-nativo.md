# ADR-0026 — A publicação de mídia vai para o core Rust; o WebView só assiste

- **Status:** Aceito
- **Data:** 2026-09-14
- **Substitui:** [ADR-0021](0021-seletor-de-tela-e-o-do-chromium.md)
- **Refina:** [ADR-0001](0001-midia-no-webview.md) (passa a valer só para a visualização),
  [ADR-0005](0005-modulo-unico-de-midia.md) (o módulo único muda de linguagem)
- **Torna desnecessário:** o transporte por IPC do [ADR-0014](0014-audio-por-aplicativo.md)

## Contexto

O [ADR-0021](0021-seletor-de-tela-e-o-do-chromium.md), de dois dias atrás, rejeitou
exatamente esta migração. O dono do projeto reverteu a decisão: seletor próprio, ausência
da barra do Chromium e áudio sem o Discord deixaram de ser acabamento e passaram a ser
requisito do S7 e do S9.

Isso por si só bastaria para reabrir o assunto. Mas a investigação feita para executá-la
mostrou que **a estimativa do ADR-0021 estava errada**, e vale registrar em que:

- O ADR-0021 contava com escrever captura Windows.Graphics.Capture à mão — enumeração de
  monitores e janelas, `IGraphicsCaptureItemInterop`, pool de frames D3D11, textura de
  staging, leitura de volta. Era a maior parte do custo estimado. **Não é preciso:** o
  `webrtc-sys` que vem com o SDK já expõe `DesktopCapturer`, compilado com
  `RTC_ENABLE_WIN_WGC`, com `get_source_list()` devolvendo id e título de cada tela e
  janela, `select_source()`, cursor opcional e frames BGRA com stride. A camada segura
  `livekit::webrtc::desktop_capturer` embrulha tudo isso em Rust sem `unsafe`.
- `TrackPublishOptions` do SDK Rust tem `simulcast`, `simulcast_layers` customizáveis,
  `video_codec: VP9`, `scalability_mode` e `degradation_preference`. É paridade com o que
  `desktop/src/media/tracks.ts` faz hoje, não um subconjunto.
- `VideoEncoderBackend` aceita `Hardware` e `Nvenc`. O encoder por hardware deixa de ser
  algo que o Chromium escolhe por nós e passa a ser algo que podemos pedir.

Foi feito um spike de build antes de qualquer decisão. O SDK compila e linka no Windows
MSVC com a toolchain do projeto.

## Decisão

**A aquisição, a codificação e a publicação de mídia acontecem no core Rust**, com o SDK
Rust do LiveKit (`livekit` 0.9). **A visualização continua no WebView2**, com
`livekit-client`, `adaptiveStream` e `dynacast` intactos.

O core Rust é um motor de mídia burro: recebe URL, token, id da fonte e preset, e publica.
Quem fala com a nossa API e resolve autenticação continua sendo o TypeScript — o token de
publicação é buscado pelo `ApiClient` e passado para o Rust por IPC. O Rust nunca conhece
nossa API REST.

O caminho de vídeo é `DesktopFrame` (BGRA) → `argb_to_nv12` → `NV12Buffer` → escala
opcional → `NativeVideoSource`. A captura é *pull*: `capture_frame()` é chamado pelo nosso
relógio, o que dá o controle exato de fps que o RF-36 pede.

## Consequências

- **O seletor de fonte passa a ser nosso**, e a barra "você está compartilhando" do
  Chromium some — porque `getDisplayMedia` deixa de ser chamado. Era o requisito.
- **O transporte de áudio por IPC do ADR-0014 é apagado antes de ser escrito.** WASAPI e
  o encoder passam a viver no mesmo processo e no mesmo domínio de relógio. A deriva entre
  o relógio do WASAPI e o do `AudioContext`, registrada no ADR-0014 como "o trabalho nativo
  de maior incerteza do roadmap", simplesmente deixa de existir. É o maior ganho desta
  mudança, e o ADR-0021 não o pesou.
- **Compartilhar deixa de derrubar o que você está assistindo.** Hoje `startShare` chama
  `connect(true)`, que reconecta a sala inteira para trocar o token — e com isso descarta
  todas as assinaturas e decodificadores. Com a publicação em outra conexão, a conexão de
  visualização nunca é tocada.
- `desktop/src/media/tracks.ts` deixa de adquirir mídia. A escada de qualidade passa a
  viver no Rust, junto de quem codifica. O módulo continua existindo para a metade do
  espectador (`applyQuality`).
- **Custo de build:** `webrtc-sys` baixa ~114 MB e compila C++ na primeira vez. O
  `desktop/src-tauri` já está fora do workspace, então o servidor não paga nada disso.
- **Duas SDKs de mídia para manter**, exatamente como o ADR-0021 advertiu. Continua sendo
  verdade; deixou de ser decisivo.
- O [ADR-0019](0019-versoes-do-livekit-sao-um-par.md) passa a ter **três** pontas para
  manter em par, não duas: servidor, `livekit-client` e `livekit` (Rust).
- **Armadilha de build no Windows, descoberta no spike e não óbvia:** os cabeçalhos do
  libwebrtc extraído ficam a ~250 caracteres de profundidade dentro de `target/`. Num
  diretório de projeto fundo, `cl.exe` estoura o `MAX_PATH` de 260 e falha com
  `C1083: cannot open include file` apontando para cabeçalhos que existem. O erro não diz
  nada sobre tamanho de caminho. Fica registrado para não custar a mesma hora de novo.

## Alternativas rejeitadas

- **Capturar no Rust e mandar os frames crus para o JS** (via `MediaStreamTrackGenerator`,
  WebSocket local ou protocolo customizado). 1080p60 em NV12 são ~186 MB/s, e ainda
  acrescenta duas cópias e um `VideoFrame` por quadro no coletor de lixo. É a mesma conta
  que o ADR-0001 usou para proibir vídeo no IPC; ela não mudou.
- **Levar também a visualização para o Rust**, renderizando em janela nativa. Jogaria fora
  a interface React inteira e obrigaria a reimplementar `adaptiveStream` e `dynacast`, que
  são o que segura o orçamento de egress (RF-32).
- **Dispositivo de vídeo virtual.** Exige driver assinado; mesma rejeição do ADR-0014.
- **Continuar com o `getDisplayMedia` e esconder a barra por `additionalBrowserArgs`.**
  Era o experimento barato autorizado pelo ADR-0021. Resolveria, na melhor das hipóteses,
  a barra — nunca o seletor nem o áudio.
