# ADR-0014 — Áudio por aplicativo via WASAPI, transportado por IPC

- **Status:** Aceito
- **Data:** 2026-09-12
- **Refina:** [ADR-0001](0001-midia-no-webview.md)
- **Refinado por:** [ADR-0025](0025-audio-exclui-o-discord.md) — o modo correto é
  *excluir* a árvore do Discord, não *incluir* o aplicativo compartilhado

## Contexto

O [ADR-0012](0012-midia-unidirecional.md) deixa a voz no Discord e a tela conosco, e com
isso cria um problema que nenhum dos dois produtos tinha sozinho.

O Chromium só oferece captura de "áudio do sistema", e só em captura de tela inteira. O
áudio do sistema do compartilhador inclui a saída do Discord, isto é, a voz de todos os
outros participantes. Compartilhar com áudio devolve, para cada espectador, a própria voz
com o atraso do caminho de mídia. Fone de ouvido não resolve — a captura de loopback é
digital e pega o mix que o sistema está reproduzindo, independentemente do dispositivo de
saída.

É o pior tipo de defeito: não aparece em teste com uma máquina só, e torna inutilizável
justamente o caso de uso central (assistir gameplay com o áudio do jogo).

## Decisão

No Windows, o áudio compartilhado é capturado **por processo**, no core Rust, via WASAPI
process loopback (`ActivateAudioInterfaceAsync` com
`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`, disponível a partir do Windows 10 build
19041). O usuário escolhe o aplicativo cujo áudio quer transmitir; a voz do Discord fica
de fora por construção, não por configuração.

O PCM capturado atravessa o IPC do Tauri até o WebView, onde é injetado como track de
áudio via `AudioWorklet` + `MediaStreamAudioDestinationNode` e publicado pelo LiveKit JS
SDK, junto do vídeo.

## Consequências

- **A exceção ao ADR-0001 é deliberada e proporcional.** Áudio a 48 kHz estéreo em f32 são
  ~384 KB/s, cerca de 0,9% dos ~41 MB/s que tornaram o IPC inaceitável para vídeo. O
  argumento que proíbe vídeo no IPC não se aplica a áudio; o vídeo continua inteiramente
  no WebView.
- É o diferencial mais claro do produto sobre o Discord e sobre qualquer ferramenta
  baseada em navegador: só um aplicativo nativo consegue fazer isso.
- Zona de revisão humana obrigatória (`CLAUDE.md` §10): é código de captura de mídia com
  API de sistema operacional.
- **É também o trabalho nativo de maior incerteza do roadmap** — buffer, relógio, deriva
  entre o relógio do WASAPI e o do `AudioContext`, e sincronia com o vídeo. Por isso fica
  na última fatia: o produto precisa funcionar sem ele.
- **Fallback normativo:** onde o process loopback não estiver disponível, cai para áudio do
  sistema inteiro com aviso explícito de que a voz dos outros será retransmitida. E o
  compartilhamento sem áudio é sempre uma opção de um clique.

## Alternativas rejeitadas

- **Aceitar o áudio do sistema com aviso** (o que a v1 fazia em RF-22c). O aviso não
  conserta o eco; só transfere a culpa para o usuário.
- **Dispositivo de áudio virtual instalado junto do aplicativo.** Resolve, mas exige driver
  assinado e instalação privilegiada — custo e risco desproporcionais.
- **Pedir que o usuário mova a voz para o nosso produto.** Contradiz o
  [ADR-0012](0012-midia-unidirecional.md) e reconstrói o Discord.
- **SDK Rust nativo do LiveKit para publicar o áudio direto do core.** Resolveria sem IPC,
  mas traria o vídeo junto para o caminho nativo, revertendo o ADR-0001 por um problema
  que só existe no áudio.
