# ADR-0001 — Mídia no LiveKit JS SDK dentro do WebView2

- **Status:** Aceito, refinado pelo [ADR-0014](0014-audio-por-aplicativo.md)
- **Data:** 2026-08-29 (originado no SRS v1.2 §2.1)

## Contexto

O cliente é um shell Tauri v2 com WebView2 (Chromium) no Windows. Existiam dois caminhos
para publicar e receber mídia: o SDK JavaScript do LiveKit rodando dentro do WebView, ou
o SDK Rust nativo no core, atravessando IPC até a camada de apresentação.

## Decisão

Toda aquisição, publicação e assinatura de **vídeo** acontece no LiveKit JS SDK dentro do
WebView2.

## Consequências

- Frames de vídeo nunca atravessam IPC. Um stream 720p I420 a 30 fps são ~41 MB/s; a
  serialização por IPC dominaria o custo de CPU do aplicativo inteiro.
- `adaptiveStream` e `dynacast` vêm prontos, e são o mecanismo que mantém o egress dentro
  do orçamento — reimplementá-los no caminho nativo seria trabalho puro de perda.
- Em troca, ficamos limitados ao que o Chromium expõe: sem captura de áudio por aplicativo,
  sem escolha fina de encoder. O ADR-0014 abre uma exceção estreita e deliberada para
  áudio, cujo volume de dados (~384 KB/s) não tem o mesmo problema.

## Alternativas rejeitadas

- **SDK Rust nativo + IPC para tudo.** Custo de IPC proibitivo para vídeo, e reimplementação
  do controle de congestionamento adaptativo.
