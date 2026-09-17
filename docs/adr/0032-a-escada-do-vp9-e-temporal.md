# ADR-0032 — A escada do VP9 é temporal (`L1T3`), não espacial

- **Status:** Aceito
- **Data:** 2026-09-17
- **Substitui a decisão de codificação registrada no** [ADR-0026](0026-publicacao-no-rust-nativo.md)
  **e limita o alcance do** [ADR-0023](0023-quem-publica-escolhe-resolucao-e-fps.md)

## Contexto

O produto oferece 1080p60 como preset (RF-36). Ele não entregava 1080p60.

Com a publicação como estava — `simulcast: true`, `scalability_mode: None`, VP9 —, uma captura
de 60 quadros por segundo chegava ao espectador como **11**. O número não aparecia em lugar
nenhum do lado de quem transmite: a captura produzia 60/s, o encoder **aceitava** 60/s, o
`quality_limitation_reason` dizia `none`, o transporte era UDP com 1 ms de RTT em loopback e
o controle de congestionamento anunciava 1,7 Mb/s livres. Todas as medições do publicador são
compatíveis com sucesso; só contando quadro no espectador o defeito aparece.

Foi isso que produziu, na prática, um relatório de "qualidade comicamente baixa" depois do
deploy na Oracle e uma investigação de rede inteira — quando a rede estava certa.

### O que foi medido

`sweep_publish_options_against_a_real_subscriber`, em `publisher.rs`: publica a mesma tela
real várias vezes, variando só as opções de publicação, e conta no espectador. Máquina de
teste: i5-10400 (6 núcleos), SFU de desenvolvimento em loopback, tela 1080p, 12 s por
variante depois de 4 s de aquecimento. `cpu %` é percentual de **um** núcleo, do processo
inteiro (captura, conversão e encoder).

| variante | fps no espectador | resolução recebida | cpu % |
|---|---|---|---|
| vp9 + escada padrão (**o que estava no ar**) | **10,8** | 1920×1080 | 54 |
| vp9 + escada personalizada de 720p30 | 10,9 | 1920×1080 | 53 |
| vp9 SVC `L2T3_KEY` | **0,0** | — | 40 |
| vp9 SVC `L3T3_KEY` | **0,0** | — | 34 |
| vp9 sem escada nenhuma | 59,7 | 1920×1080 | 114 |
| **vp9 SVC `L1T3`** | **59,2** | **1920×1080** | **133** |
| vp8 + escada padrão | 59,3 | 1920×1080 | 157 |
| vp8 + escada de 960×540@30 | 42,3 | 960×540 | 258 |
| h264 + escada padrão | 53,3 | 1920×1080 | 176 |
| h264 sem escada, pedindo encoder de hardware | 45,6 | 1920×1080 | 190 |

Dois achados independentes da nossa configuração, e que valem mais que a tabela:

1. **As duas formas de pedir uma escada espacial ao VP9 estão quebradas nesta pilha**, e as
   duas falham em silêncio. Simulcast (mais de uma codificação RTP) degrada para 11 fps;
   SVC espacial entrega zero quadro. Não é escolha de bitrate nem de CPU — a variante de
   11 fps roda a 54 % de um núcleo, metade do que a versão que funciona consome.
2. **Não existe encoder de hardware neste build.** `VideoEncoderBackend::list_available()`
   responde `[Auto, Software, PreEncoded]`. Pedir `Hardware` não falha: cai de volta no
   software, e mediu *pior*. O `hardware_encoder: false` do painel é estrutural, não
   circunstancial, e não há tecla a apertar ali.

O ADR-0026 registrava a metade certa disto — que `L3T3_KEY` fazia a publicação sair pelo
ralo — e tirava a conclusão errada, de que *nenhum* `scalability_mode` servia. A diferença
entre `L3T3_KEY` e `L1T3` é a diferença entre o produto funcionar e não funcionar.

## Decisão

**A publicação usa VP9 com `scalability_mode: "L1T3"` e `simulcast: false`.** Uma camada
espacial, três temporais.

Medido depois da mudança, mesma bancada: **59,5 fps a 1920×1080 no espectador**, estável ao
longo de 20 s, com 5,8 Mb/s de banda disponível sobrando.

VP9 continua sendo o codec apesar de custar mais CPU que o VP8 porque gasta cerca de
**metade do bitrate** para a mesma tela (584 kb/s contra 1083 kb/s na varredura). Em quem
transmite isso é a diferença entre caber e não caber numa subida residencial; no servidor é
egress, que é o recurso escasso do plano gratuito da Oracle.

## Consequências

**Fica fácil:** 1080p60 é verdade. E o espectador que não aguenta o relógio inteiro continua
tendo para onde cair — o SFU corta camada temporal e entrega 30 ou 15 fps na resolução
cheia, sem renegociar nada.

**Fica caro:** cerca de 1,3 núcleo em quem transmite a 1080p60. Num i5-10400 são 11 % da
máquina; numa máquina de quatro núcleos rodando um jogo, é bastante. É o preço de não haver
encoder de hardware, e o preset de 1080p30 (105 %) e o de 720p existem por causa disso.

**Fica perdido:** a economia **espacial** de egress do RF-32. Um espectador com o ladrilho
pequeno na grade não recebe mais uma camada de resolução menor, porque não existe uma — ele
recebe 1080p e o `adaptiveStream` não tem o que escolher. O que sobrou é a escada temporal.
Isto não é uma escolha de projeto; é o que esta pilha entrega. Se o `webrtc-sdk` consertar o
simulcast de VP9, o caminho de volta é uma linha, e a varredura está no repositório para
dizer quando isso aconteceu.

**Fica proibido:** trocar `L1T3` por `L2T3_KEY`, `L3T3_KEY` ou `simulcast: true` sem rodar a
varredura antes. Os três modos falham em silêncio, com o painel do publicador dizendo que
está tudo bem.

## Alternativas rejeitadas

**VP8 com a escada padrão** (59,3 fps, escada espacial de verdade). Recuperaria o RF-32, e
custa 157 % de núcleo contra 133 %, com quase o dobro do bitrate para a mesma imagem. Texto
em 1080p é onde o VP8 fica visivelmente atrás. Rejeitado pelo bitrate, que é o recurso que o
servidor paga.

**H.264** (53,3 fps, 176 %). Mais caro e mais fraco que os dois em conteúdo de tela — o
próprio OpenH264 avisa, no log, que desliga quantização adaptativa e detecção de fundo para
`screen content`. O que o justificaria seria encoder de hardware, que não existe aqui.

**Escada de simulcast personalizada** (`simulcast_layers`), para trocar a camada de baixo de
3 fps por algo assistível. Medida: 10,9 fps. O defeito é do caminho de múltiplas
codificações no VP9, não dos números da escada.

**Continuar sem escada nenhuma** (`simulcast: false`, sem `scalability_mode`): 59,7 fps a
114 % — mais barato que o `L1T3`. Rejeitado por 19 pontos de CPU: sem as camadas temporais,
o espectador com banda ruim não tem degradação nenhuma para onde ir, e a única saída do SFU
passa a ser parar de mandar.
