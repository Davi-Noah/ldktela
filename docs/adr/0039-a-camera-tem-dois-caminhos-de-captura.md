# ADR-0039 — A câmera tem dois caminhos de captura: Media Foundation e DirectShow

- **Status:** Aceito
- **Data:** 2026-09-23
- **Altera:** a decisão 8 do [ADR-0038](0038-camera-e-uma-segunda-publicacao.md), que dizia
  "a captura é nossa, em Media Foundation". Continua sendo nossa; deixa de ser só em Media
  Foundation. O resto do 0038 fica intacto.

## Contexto

O 0038 escolheu Media Foundation e escreveu o porquê no cabeçalho do módulo: é a API que o
Windows mantém viva, é ela que responde pela configuração de privacidade da câmera, é o
frame server que deixa dois aplicativos lerem um dispositivo, e é dela que vêm os
conversores de formato que nos pouparam de escrever um decodificador. Nada disso ficou
falso.

O que ficou falso foi a premissa de que **toda câmera fala Media Foundation**.

Na primeira vez que o recurso encontrou uma máquina de usuário, as câmeras não abriram.
O quadro medido, contra o DroidCam desta máquina:

| O que foi tentado | Resultado |
|---|---|
| `MFEnumDeviceSources` | encontra os dispositivos, com nome e link simbólico |
| `MFCreateDeviceSource` pelo link simbólico | `E_INVALIDARG` (`0x80070057`) |
| `IMFActivate::ActivateObject`, o caminho canônico | `E_INVALIDARG` |
| Em thread MTA limpa, com `MFStartup` novo | `E_INVALIDARG` |
| Categorias `VIDEO_CAMERA` e `CAPTURE` | `E_INVALIDARG` |
| Aplicativo **Câmera** do Windows, que é só MF | erro; nenhuma câmera oferecida |

Ao mesmo tempo, as mesmas câmeras entregam vídeo no Google Meet, e o Meet lista **quatro**
onde o Media Foundation enumera **duas** — as que faltam incluem a OBS Virtual Camera, que
é um filtro DirectShow puro.

A conclusão é que essas câmeras **existem para o Media Foundation e não abrem por ele**.
São câmeras virtuais: DroidCam, OBS, Iriun, EpocCam, ManyCam. Elas registram um filtro
DirectShow, e algumas registram também um dispositivo KS que a enumeração do MF encontra
mas cuja ativação falha. O Chromium não tem esse problema porque implementa os dois
caminhos e cai para o DirectShow quando o primeiro não serve.

**Isso não é caso de borda para este produto.** O público é de quem já transmite: OBS e
câmera de celular são o normal, não a exceção. Uma câmera que funciona no Discord e no Meet
e não funciona aqui é, para quem usa, um defeito nosso — e a única razão de a câmera estar
no escopo ([ADR-0038](0038-camera-e-uma-segunda-publicacao.md)) é fazer melhor do que o
Discord faz.

## Decisão

1. **Dois caminhos de captura, com o Media Foundation na frente.** O MF continua sendo o
   preferido por tudo o que o 0038 listou. O DirectShow é o caminho alternativo, e existe
   para os dispositivos que o MF não abre.

2. **A lista de câmeras é uma só, unificada.** Os dois caminhos enumeram, e o resultado é
   fundido: um dispositivo visto pelos dois aparece **uma vez**. Expor "DroidCam (MF)" e
   "DroidCam (DirectShow)" seria vazar a nossa implementação para dentro de um menu em que a
   pessoa só quer escolher um rosto.

3. **A fusão casa por caminho de dispositivo, depois por nome.** O nome de exibição de um
   moniker DirectShow de dispositivo físico contém o mesmo link simbólico que o MF usa, e é
   ele que casa os dois. Câmeras virtuais sem caminho de dispositivo casam pelo nome
   amigável, que para elas é distintivo.

4. **A escolha do caminho acontece ao abrir, não ao listar.** Tentar abrir cada dispositivo
   durante a enumeração para descobrir quem responde custaria segundos e ligaria o LED de
   todas as câmeras da máquina toda vez que o menu abrisse. Então a lista é otimista: ela
   mostra o dispositivo, e o caminho se resolve quando ele é escolhido.

5. **A queda para o DirectShow é automática e silenciosa.** Quando o MF recusa um
   dispositivo que ele mesmo acabou de enumerar, tentamos o DirectShow com o dispositivo
   correspondente antes de reportar erro. Quem escolheu uma câmera quer a câmera, não um
   aviso sobre qual API do Windows a abriu. Se os dois recusarem, o erro reportado é o **do
   Media Foundation**: ele é o caminho preferido, e o código dele é o que se pesquisa.

6. **O destino do grafo DirectShow é um filtro nosso**, implementando `IBaseFilter`, `IPin`
   e `IMemInputPin`, e não o `ISampleGrabber`. O sample grabber seria muito menor, mas está
   aposentado: a Microsoft o tirou do SDK, ele não existe nos bindings que usamos, e ele
   mora no `qedit.dll`, que faz parte do Media Feature Pack e **pode não estar na máquina**.
   Uma câmera que falha em Windows N por falta de uma DLL aposentada é exatamente o defeito
   que este ADR existe para não repetir. O Chromium escreveu o próprio filtro pela mesma
   razão.

7. **A conversão de cor é nossa, porque o DirectShow não tem o conversor do MF.** O caminho
   MF pede NV12 e recebe NV12, com o `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING` inserindo o
   que faltar. No DirectShow negociamos o formato **que o dispositivo já emite** e
   convertemos aqui: NV12, I420/IYUV, YUY2, UYVY, RGB32 e RGB24.

8. **Formato comprimido não entra.** Se o dispositivo só oferecer MJPEG ou H.264,
   recusamos com mensagem própria em vez de deixar o DirectShow montar um decodificador
   por conta. Um grafo montado por adivinhação insere filtros de terceiros no nosso
   processo — o conector "inteligente" do DirectShow é conhecido por isso — e o custo de
   depurar o que ele escolheu supera o de não suportar um dispositivo raro. Toda câmera que
   emite MJPEG emite também YUY2 em alguma resolução.

9. **O caminho DirectShow é empurrado, e não puxado.** O MF bloqueia em `ReadSample` numa
   thread nossa; o DirectShow chama `IMemInputPin::Receive` na thread de streaming do
   próprio dispositivo. Os dois terminam no mesmo lugar — um `Frames` que converte, escala
   e entrega ao encoder —, e é esse ponto de encontro que mantém a diferença contida num
   módulo só.

10. **Um dispositivo perdido é evento, não silêncio.** O MF descobre pela flag de fim de
    fluxo; o DirectShow, pelo `EC_DEVICE_LOST` na fila de eventos do grafo. Os dois chamam
    o mesmo `on_lost`, e quem compartilha vê a transmissão terminar com motivo.

## Consequências

- Uma câmera virtual que só fala DirectShow passa a funcionar, que é o objetivo.
- O módulo `camera.rs` vira o diretório `camera/`, com `mf.rs`, `dshow.rs` e `convert.rs`
  em volta de uma API pública que não mudou. Continua inteiro na zona de revisão humana
  (`CLAUDE.md` §10).
- Ganhamos ~700 linhas de COM inseguro, das quais a maior parte é um filtro DirectShow que
  precisa estar certo em detalhes que não aparecem em teste sem hardware: contagem de
  referência entre filtro e pino, quem é dono de qual `AM_MEDIA_TYPE`, e o ciclo que o pino
  não pode criar com o grafo. É a parte deste ADR que mais pode dar errado, e é por isso que
  o teste contra a câmera de verdade — `#[ignore]`, como o teste de SFU real — é parte da
  entrega e não um extra.
- A configuração de privacidade de câmera do Windows não governa o caminho DirectShow como
  governa o MF. Quem bloqueia a câmera no Windows e mesmo assim a vê funcionar por uma
  câmera virtual não está furando a nossa porta: está usando um filtro de software que
  nunca passou por aquela configuração, e isso vale igual para o Discord, o Meet e o OBS.
- Não passamos a suportar captura de áudio por DirectShow, nem qualquer outro uso de grafo.
  O que entra é uma câmera, com um filtro de destino, e nada além disso.

## Alternativas rejeitadas

- **Só Media Foundation, e documentar a limitação.** É o estado de hoje. Deixa de fora as
  câmeras que o público efetivamente usa, num recurso cuja razão de existir é ser melhor
  que a alternativa. Foi a premissa que a primeira máquina de usuário derrubou.
- **`ISampleGrabber` do `qedit.dll`.** Cerca de 60 linhas em vez de 600. Rejeitado pela
  decisão 6: interface aposentada, ausente dos bindings, e numa DLL que pode não existir.
- **Pedir que a pessoa configure a câmera virtual em "modo Media Foundation".** O DroidCam
  não tem esse modo; a OBS Virtual Camera não tem esse modo. Exigir configuração que não
  existe é recusar com passos extras.
- **Usar o `getUserMedia` do WebView e mandar os quadros ao Rust.** O Chromium resolveria a
  enumeração sozinho, e é tentador. Contraria o [ADR-0026](0026-publicacao-no-rust-nativo.md)
  — nada no WebView adquire mídia —, e passar vídeo cru por IPC a 30 fps é justamente o
  custo que aquele ADR existe para não pagar.
