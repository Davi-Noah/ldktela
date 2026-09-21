# Consumo de hardware do cliente: onde ele está e o que dá para fazer

Resposta à issue #12 — *analisar a viabilidade de otimizar o consumo de hardware do cliente ao
transmitir e assistir*.

Documento de análise, não de decisão. Nada aqui foi implementado; o que virar trabalho vira ADR ou
entrada em `DECISIONS.md` antes de virar código.

---

## 1. O que já está medido

Tudo abaixo já estava no repositório ou foi medido para este documento. Percentuais de CPU são de
**um núcleo**, salvo onde diz o contrário.

| onde | custo | fonte |
|---|---|---|
| Encode 1080p60 VP9 `L1T3` | **133% de um núcleo** | varredura do [ADR-0032](adr/0032-a-escada-do-vp9-e-temporal.md), `RESULTS.md` |
| Encode 1080p60 VP8 | 157% | idem |
| Encode 1080p60 H.264 | 176%, e **190% pedindo encoder de hardware** | idem |
| Transmissão sustentada (processo inteiro, 30 min) | mediana 84% de um núcleo, p95 110%, máx 141% | `RESULTS.md` §medição de 30 min |
| RSS publicando (app + 7 processos do WebView2) | mediana 585 MB, p95 621 MB | idem |
| *Preview* local da própria tela, modo grade (960×540 @ 6 fps) | redução 0,80 ms/quadro · **JPEG 6,67 ms/quadro** · 74 KB/quadro · 448 KB/s de IPC | `preview_costs_what_it_is_worth`, release, hoje |
| *Preview* local, modo foco (1600×900 @ 12 fps) | redução 3,96 ms/quadro · **JPEG 16,88 ms/quadro** · 180 KB/quadro · 2,7 MB/s de IPC | idem |

Duas leituras que só aparecem quando se multiplica:

- **O preview em foco custa ~20% de um núcleo só para codificar JPEG** (16,88 ms × 12 quadros/s ≈
  202 ms por segundo), mais a decodificação desses mesmos JPEGs do outro lado, no WebView. No modo
  grade cai para ~4%.
- **Não existe encoder de hardware neste caminho.** A varredura do ADR-0032 mediu H.264 *pedindo*
  encoder de hardware ficando **pior** que o software (190% contra 176%): o libwebrtc pré-compilado
  do SDK expõe `[Auto, Software, PreEncoded]` e não usa NVENC, QSV ou AMF.

---

## 2. Onde o custo está, por caminho

**Quem transmite** paga, em ordem de tamanho:

1. **Encode VP9** — ~1 núcleo a 1080p60. É o piso do desenho atual.
2. **Preview local** — 4% (grade) a 20% (foco) de um núcleo, mais IPC e decodificação no WebView.
3. **Captura e conversão de cor** — sai do `DesktopCapturer` do libwebrtc e vira NV12; medido em
   conjunto com o encode, não isolado.
4. **Áudio** — WASAPI a 48 kHz estéreo. Ruído estatístico perto do resto.

**Quem assiste** paga **por tela assinada**: uma decodificação VP9 e uma composição no WebView2
para cada uma. É linear no número de telas, e era obrigatório até as issues #6 e #7 — todo mundo
recebia tudo.

---

## 3. Candidatos, do mais barato ao mais caro

### a. Já entregue: recusar o que não se quer ver

As issues #6 e #7 ([ADR-0036](adr/0036-assinar-uma-tela-e-escolha-de-quem-assiste.md)) permitem
sair de uma tela, o que cancela a assinatura de verdade. Numa sala com três transmissões, quem
assiste a uma corta dois terços do trabalho de decodificação. **É a maior economia disponível hoje
e já está no produto** — falta medir quanto, o que o §4 propõe.

### b. Barato, não medido: o preview em foco

20% de um núcleo é muito para uma imagem que existe só para a pessoa conferir o que está mandando.
Três caminhos, em ordem de esforço:

- **Baixar `FOCUS_FPS` de 12 para 6 ou 8.** Uma linha em `config.ts`/`preview.rs`. O preview não
  precisa ser fluido: ele é conferência, não a transmissão (ADR-0030).
- **Baixar `FOCUS_QUALITY`** de 80. JPEG a 70 custa menos e a diferença é invisível no tamanho em
  que o preview aparece.
- **Trocar data URL por outro transporte.** 2,7 MB/s de string base64 atravessando o IPC do Tauri é
  o desenho mais simples possível, e foi a escolha certa para começar (ADR-0030). Um `SharedArrayBuffer`
  ou um canal binário evitaria a codificação JPEG inteira — é a mudança grande deste grupo, e a
  única que ataca os 16,88 ms em vez de reduzir quantas vezes eles acontecem.

### c. Médio: presets e o que o usuário realmente precisa

1080p30 corta perto da metade do trabalho de encode de 1080p60, e 720p60 corta a área. Os presets
já existem (RF-36) e a escolha é de quem publica (ADR-0023). O que **não** existe é o produto
dizendo, em cima da hora, que a máquina está no limite — o dado já vem do encoder
(`limited_by: "cpu"`), e hoje só aparece como estatística. Sugerir "sua máquina não está dando
conta; experimente 1080p30" quando o encoder disser `cpu` é barato e resolve o caso real.

### d. Caro e incerto: encoder de hardware

É o item de maior ganho teórico — "a diferença entre um núcleo e quase nada", como o próprio
`publisher.rs` registra — e o de maior risco:

- O libwebrtc pré-compilado do `livekit` 0.9 não oferece o caminho. Usá-lo exige **compilar o
  libwebrtc** com os encoders de hardware habilitados, ou publicar quadros já codificados
  (`PreEncoded`) produzidos por Media Foundation/NVENC fora do libwebrtc.
- A segunda via muda o desenho do publicador: passaria a existir um encoder nosso, com controle de
  bitrate, camadas temporais e resposta a congestionamento **fora** do controle do libwebrtc —
  exatamente o que o ADR-0032 mostrou ser delicado.
- H.264 por hardware também mudaria o codec da sala, e o ADR-0032 escolheu VP9 com medição.

Não recomendo começar por aqui. Recomendo **medir primeiro** se o encode é mesmo o incômodo real
dos usuários, ou se o que incomoda é a soma preview + decodificação de N telas, que custa muito
menos para atacar.

### e. Não fazer: reduzir a taxa de quadros de quem assiste

`adaptiveStream` e a escolha de camada por ladrilho já fazem isso, e a escada é temporal
(ADR-0032): cortar mais vira o "slideshow" que o produto existe para não ser.

---

## 4. O que medir antes de decidir

O repositório já tem as ferramentas; faltam três cenários.

1. **Assistir N telas.** Não existe medição do lado do espectador com 1, 2, 3 e 4 telas assinadas.
   É o número que diz quanto as issues #6 e #7 economizam de verdade, e é o que decide se o esforço
   vai para o encode ou para a decodificação.
2. **Transmitir com e sem preview, e com preview em foco.** `process_cpu_seconds`, em
   `publisher.rs`, já mede CPU do processo; basta rodar as três variantes com a mesma tela.
3. **Repetir a varredura do ADR-0032 numa máquina fraca.** Todos os números vêm de um i5-10400 de
   6 núcleos. "Dá conta" ali não diz nada sobre a máquina de quem reclama, e a issue #12 nasceu de
   uma reclamação.

Sem (1) e (2), qualquer otimização é palpite — inclusive as que este documento chama de baratas.
