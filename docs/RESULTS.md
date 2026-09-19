# Resultados medidos

Números reais, não estimativas. Cada seção diz **como** foi medido, porque o
método muda o que o número significa.

---

## Fase 2 — duas máquinas, caminho direto (2026-09-14)

Primeira medição real do produto: UDP direto, LAN.

> Escrito como "o piso, não a condição que o RNF-02 especifica". Deixou de ser
> piso em 2026-09-14: sem bloqueio de rede na região alvo, o caminho direto **é**
> o de produção para a maioria dos usuários
> ([ADR-0020](adr/0020-transporte-comum-sem-adversario-de-rede.md)). O relay passa a
> valer só para quem está atrás de CGNAT.

### Arranjo

| | |
|---|---|
| Publicador | Aplicativo no host Windows 11, 12 núcleos lógicos, 1920×1080 |
| Espectador 1 | Aplicativo em **Windows Sandbox**, na mesma máquina |
| Espectador 2 | Sonda headless (Edge, `--disable-gpu`), na mesma máquina |
| SFU | `livekit-server:v1.13.6` em Docker, `node_ip` na LAN |
| Codec | VP9 |
| Duração | ~33 min, em duas fases |

**As três pontas rodam na mesma máquina física.** Isso mata a rede como
variável — o RTT medido foi de 2–3 ms — e por isso os números de latência e
bitrate são limpos. Em troca, cria contenção de CPU que contamina os números de
**qualidade** (ver "Congelamentos").

Duas fases, porque um conteúdo só não serve para as duas coisas:

- **Fase A** (~8 min): tela cheia com o relógio de calibração. Necessária para
  medir latência; também é quase o pior caso de codec, com o quadro inteiro
  mudando sempre.
- **Fase B** (~26 min): conteúdo real, com movimento. É a base do orçamento de
  egress.

### Método da latência

A [DESTRAVAR.md §2.4](DESTRAVAR.md) descreve medir glass-to-glass fotografando
dois relógios com o celular, 20 vezes, e tirando o p95. Foi substituído por
medição automática, com três ordens de grandeza mais amostras:

Uma página pinta o horário corrente em **17 blocos binários** (16 bits de
milissegundos + 1 de paridade) no topo da tela compartilhada. A sonda desenha
cada quadro recebido num canvas, lê os 17 pixels, confere a paridade — que
descarta leitura feita no meio da troca de quadro — e compara com o próprio
relógio.

Isso só é válido porque **publicador e sonda rodam na mesma máquina**: não há
desvio de relógio entre os dois. A medida cobre captura → encode → SFU → decode
→ render em canvas. Fica de fora apenas a latência do painel físico dos dois
lados, que a foto incluiria; é o mesmo alvo com uma parcela fixa a menos.

### Latência (RNF-02)

**10.064 amostras**, Fase A.

| Percentil | Medido |
|---|---|
| p50 | 141 ms |
| p75 | 157 ms |
| p90 | 186 ms |
| **p95** | **212 ms** |
| p99 | 296 ms |
| min / max | 99 / 438 ms |

**RNF-02 (p95 < 300 ms): passa**, com 29% de folga.

> Uma leitura anterior, com as primeiras 1.403 amostras, deu p95 = 313 ms e
> reprovava. Estava contaminada pelo aquecimento: a sonda tinha acabado de
> conectar e o buffer de jitter ainda se acomodava. Vale como aviso de método —
> **não feche número de latência sem alguns milhares de amostras.**

Com RTT de 2–3 ms, praticamente toda essa latência é captura, encode, buffer e
decode. A rede não é o gargalo aqui, o que significa que **o caminho relayado da
Fase 3 vai somar a isso**, não substituir.

### Custo no publicador (RNF-03, RNF-04)

391 amostras em 30 min, somando a árvore de processos (o principal mais 7 do
WebView2 — só o processo principal mostra 36 MB e esconde o resto).

| Métrica | Mediana | p95 | Máx | Alvo |
|---|---|---|---|---|
| RSS agregado | 585 MB | 621 MB | 625 MB | < 700 MB publicando |
| CPU (% da máquina) | 7,0% | 9,2% | 11,7% | — |
| CPU (% de um núcleo) | 84% | 110% | 141% | — |

**RNF-03 (< 700 MB publicando): passa**, com 11% de margem. É folga curta para
um alvo que já é o mais generoso da tabela.

**RNF-04**: o orçamento de encode fica registrado em **~1 núcleo** para 1080p.
Regressão que estoure isso é defeito, não característica.

### SFU

| Métrica | Mediana | p95 | Máx |
|---|---|---|---|
| CPU | 17,9% | 35,0% | 77,2% |
| RAM | 92 MB | 94 MB | 103 MB |

Barato, como esperado de um SFU que só repassa. Confirma a premissa do
[ADR-0013](adr/0013-turn-tls-443-primario.md) de que o custo do relay é CPU e ela
sobra.

### Egress (RNF-05)

Medido pelos contadores do próprio SFU (`livekit_packet_bytes`), habilitados
neste teste. **Não é extrapolação do painel do cliente**, que só sabe o que o
cliente acha que enviou — é a exigência literal do RNF-05.

| | Entrada (publicador→SFU) | Saída (SFU→2 espectadores) | Egress |
|---|---|---|---|
| Fase A (relógio) | 2,16 Mbps | 4,30 Mbps | 1,94 GB/h |
| **Fase B (conteúdo real)** | **4,50 Mbps** | **8,73 Mbps** | **3,93 GB/h** |

A saída é 1,94× a entrada, com 2 espectadores — o SFU replica, como deveria.

**Projeção para o teto de 6 TB/mês**, a partir da Fase B (~4,4 Mbps por
espectador):

| Espectadores | Egress | Horas até o teto |
|---|---|---|
| 2 | 3,9 GB/h | ~1.530 h |
| 4 | 7,9 GB/h | ~765 h |
| **8** | **15,7 GB/h** | **~382 h** |

O SRS estimava ~21,6 GB/h e ~277 h para 8 espectadores, assumindo 6 Mbps. O
medido é **mais folgado que a estimativa**: 382 h contra 277 h.

Tela parada derruba o consumo para ~76 kbps e 1 fps — `adaptiveStream` e
`dynacast` fazem o trabalho, e o orçamento acima é de uso ativo, não de sala
aberta ociosa.

### Recepção

Medido na sonda, via `getStats()` do WebRTC.

| Métrica | Fase A | Fase B |
|---|---|---|
| Resolução | 1920×1080 em 100% das amostras | idem |
| fps (mediana) | 30 | 19 |
| Recebido (mediana) | 2.438 kbps | 4.831 kbps |
| Jitter (mediana / p95) | 4 / 9 ms | 13 / 35 ms |
| Buffer de jitter (p95) | 66 ms | 184 ms |
| RTT (mediana / máx) | 2 / 12 ms | 3 / 124 ms |
| Pacotes perdidos | 0 | 83 |
| **Congelamentos** | 15 (12,1 s) | **210 (138,9 s)** |

A resolução nunca degradou: 1080p do começo ao fim, nas duas fases.

### Congelamentos — o número que eu não confio

139 segundos de congelamento em 26 minutos é ~9% do tempo. Se fosse um número
limpo, seria reprovação do produto. **Não é um número limpo**, e a explicação
mais provável não é o produto:

- A sonda decodifica **VP9 1080p em software** (`--disable-gpu`).
- As três pontas — publicador codificando com ~1 núcleo, VM do Sandbox,
  e a sonda decodificando — disputam a **mesma** máquina.
- O buffer de jitter subindo de 22 ms para 184 ms e o RTT saltando para 124 ms
  numa LAN são assinaturas de **contenção de CPU**, não de rede: numa rede ruim
  o que sobe primeiro é a perda, e a perda ficou em 83 pacotes num teste inteiro.
- O espectador humano, no Sandbox, relatou imagem funcionando.

**O que isso exige:** repetir com o espectador numa máquina separada e com
decode acelerado antes de tratar como defeito ou como aprovação. Fica registrado
como **medição inconclusiva**, não como aprovação.

### Veredito da Fase 2

| RNF | Alvo | Medido | |
|---|---|---|---|
| RNF-02 latência | p95 < 300 ms | **212 ms** | ✅ |
| RNF-03 memória | < 700 MB publicando | **585 MB** (p95 621) | ✅ |
| RNF-04 encode | registrar orçamento | **~1 núcleo** | ✅ registrado |
| RNF-05 egress | 6 TB/mês | **3,93 GB/h** com 2; ~382 h com 8 | ✅ dentro |
| Qualidade | sem congelamento | 139 s em 26 min | ⚠️ inconclusivo |

O que a Fase 2 mostra é que, **quando a mídia passa**, o custo e a latência cabem
no orçamento.

> **Revisado em 2026-09-14.** Este parágrafo dizia que a premissa do produto não
> estava provada, porque o que decidiria seria a Fase 3, com UDP bloqueado. Essa
> Fase 3 foi rebaixada: não há bloqueio de rede a mídia em tempo real na região
> alvo — quem desligou o compartilhamento de tela foi o próprio Discord
> ([ADR-0020](adr/0020-transporte-comum-sem-adversario-de-rede.md)). O caminho direto
> medido aqui **é** o caminho de produção para a maioria dos usuários, e estes
> números passam a valer como aprovação, não como piso.

### A folga de latência aqui não é a folga de produção

Os 212 ms de p95 saíram com as três pontas na mesma máquina, RTT de 2–3 ms. Um
usuário real contra uma VM soma o RTT de verdade: o SRS v1.2 estimava 35–45 ms
entre Nordeste e Sudeste, o que põe o p95 em torno de **250 ms**.

Continua passando o RNF-02, com **~15% de folga em vez dos 29%** medidos. Vale
saber antes de gastar essa folga em outra coisa — um encoder mais lento, uma
camada a mais, um buffer maior.

---

## Fase 3 — Caminho relayado

**Não executada, e rebaixada em 2026-09-14.** Deixou de ser o teste que decide o
produto: não há bloqueio de rede a mídia em tempo real na região alvo
([ADR-0020](adr/0020-transporte-comum-sem-adversario-de-rede.md)).

O que resta é a verificação de CGNAT — quem está atrás de NAT restritivo só
conecta pelo relay — e ela roda junto da fatia S2, quando houver TURN.

---

## Pendente: qualidade com espectador em outra máquina

O item de maior valor do roadmap hoje, e o único número desta página que não
conclui nada. Repetir a Fase B com:

- o espectador numa **máquina física separada**;
- **decode acelerado por hardware** (sem `--disable-gpu`);
- a mesma instrumentação de `getStats()`.

Objetivo: descobrir se os 139 s de congelamento eram contenção de CPU, como a
assinatura sugere, ou defeito do produto. Enquanto não for refeito, a linha
"Qualidade" do veredito continua ⚠️.

---

## 2026-09-17 — contando quadros **no espectador**

A medição que faltava, e a que muda a conclusão das outras.

Todas as anteriores olharam o publicador: quadros capturados, bitrate enviado,
`getStats()` local. Nenhuma respondia à única pergunta que importa — quantos
quadros chegam do outro lado. E acontece que as duas coisas podem discordar
completamente sem que nada acuse: com a publicação como estava, uma captura de
60 quadros por segundo chegava ao espectador como **11**, enquanto o publicador
relatava 60 capturados, 60 aceitos pelo encoder, `quality_limitation_reason:
none` e 1,7 Mb/s de banda sobrando.

### Arranjo

| | |
|---|---|
| Publicador | Core Rust, i5-10400 (6 núcleos / 12 threads), tela 1920×1080 |
| Espectador | Segunda conexão do SDK Rust, contando quadros decodificados |
| SFU | `livekit-server:v1.13.6` em Docker, loopback (RTT 1–2 ms) |
| Instrumento | `measure_*_end_to_end` e `sweep_publish_options_against_a_real_subscriber`, em `desktop/src-tauri/src/publisher.rs` |

Loopback **de propósito**: o objetivo era isolar o codec, não a rede. O caminho
de rede real já foi medido e está saudável (`udp`, 71 ms contra a VM da Oracle).

### A varredura que achou o defeito

Mesma tela real, 12 s por variante depois de 4 s de aquecimento. `cpu %` é
percentual de **um** núcleo, do processo inteiro.

| variante | fps no espectador | resolução recebida | cpu % |
|---|---|---|---|
| vp9 + escada padrão (**o que estava no ar**) | **10,8** | 1920×1080 | 54 |
| vp9 + escada personalizada de 720p30 | 10,9 | 1920×1080 | 53 |
| vp9 SVC `L2T3_KEY` | **0,0** | — | 40 |
| vp9 SVC `L3T3_KEY` | **0,0** | — | 34 |
| vp9 sem escada | 59,7 | 1920×1080 | 114 |
| **vp9 SVC `L1T3`** (escolhido) | **59,2** | 1920×1080 | 133 |
| vp8 + escada padrão | 59,3 | 1920×1080 | 157 |
| vp8 + escada de 960×540@30 | 42,3 | 960×540 | 258 |
| h264 + escada padrão | 53,3 | 1920×1080 | 176 |
| h264, pedindo encoder de hardware | 45,6 | 1920×1080 | 190 |

Dois achados que não são sobre a nossa configuração:

1. **As duas formas de pedir escada espacial ao VP9 estão quebradas nesta pilha,
   e as duas falham em silêncio.** Ver [ADR-0032](adr/0032-a-escada-do-vp9-e-temporal.md).
2. **Não existe encoder de hardware neste build.** `VideoEncoderBackend::list_available()`
   responde `[Auto, Software, PreEncoded]`; pedir `Hardware` cai no software e
   mede pior. O `hardware_encoder: false` do painel é estrutural.

### Depois da correção

| preset | capturados | recebidos pelo espectador | resolução recebida | limite |
|---|---|---|---|---|
| 1080p60 | 60,0 /s | **59,5 /s** | 1920×1080 | nenhum |
| 1080p30 | 30,1 /s | 29,7 /s | 1920×1080 | nenhum |
| 720p60 | 59,9 /s | 58,3 /s | 1280×720 | nenhum |

Bitrate entre 130 e 600 kb/s com a tela quase parada, com teto de 6 Mb/s. O
número de egress continua sendo o da Fase B, que mediu conteúdo em movimento.

### Custo do preview local (ADR-0030)

Tela real de 1920×1080, `preview_costs_what_it_is_worth`:

| | redução (thread de captura) | JPEG (thread própria) | quadro | IPC |
|---|---|---|---|---|
| grade, 480×270 @ 3 fps | 0,4–1,0 ms | 1,5–2,7 ms | 13–17 KB | ~50 KB/s |
| foco, 1600×900 @ 12 fps | 4,2–6,5 ms | 16,8–23,5 ms | 123–169 KB | ~2 MB/s |

A faixa é máquina ociosa contra máquina ocupada. O que fixa o teto é a primeira
coluna: a redução roda **dentro** do laço de captura, que a 60 fps tem 16,6 ms
por quadro para tudo.

---

## Revisão: os 139 s de congelamento da Fase 2

A Fase 2 registrou 139 s de congelamento como **inconclusivo**, com a suspeita
de contenção de CPU — as três pontas rodavam na mesma máquina.

A suspeita provavelmente estava errada. Aquela medição rodou com a publicação em
VP9 + escada padrão, que é exatamente a combinação que esta página acaba de medir
entregando **10,8 quadros por segundo** ao espectador. Um stream a 11 fps é lido
como travando, e nada no painel do publicador teria acusado: `limitado por` dizia
`none`.

Fica como hipótese forte, e não como fato, porque a Fase 2 não separou as duas
causas. Repeti-la agora custa pouco e decide: se o congelamento sumir com o
mesmo arranjo de máquina única, era o codec.
