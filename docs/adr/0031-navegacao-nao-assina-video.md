# ADR-0031 — Nenhum elemento de navegação assina vídeo

- **Status:** Aceito
- **Data:** 2026-09-16

## Contexto

A revisão de interface de 2026-09-16 pediu paridade com o Discord. Num ponto essa paridade
é impagável: no Discord, estando em foco numa transmissão, você vê as outras como miniaturas
ao vivo numa faixa lateral e troca clicando.

Miniatura ao vivo é assinatura ao vivo. O [ADR-0023](0023-quem-publica-escolhe-resolucao-e-fps.md)
e a RF-32 já dizem que é o **layout** que decide o custo: com N telas visíveis o egress
multiplica por N, e é por isso que `adaptiveStream` e `dynacast` são obrigatórios e que o
modo foco esconde as outras telas em vez de encolhê-las.

A tentação é específica e reaparece toda vez que alguém compara as duas interfaces lado a
lado: "é só uma faixinha de 160 px". Uma faixa de 160 px assina a camada baixa de todas as
telas da sala, para sempre, enquanto a janela estiver aberta.

## Decisão

**Nenhum elemento cuja função seja navegar pode criar ou manter assinatura de vídeo.**

A única coisa que assina vídeo é o conteúdo que o usuário está olhando naquele instante.
Trocar de tela se faz por **avatar**, que é um PNG estático que a lista de participantes já
carregou: no modo foco, a barra de controles mostra fichas com o avatar de cada publicador,
e clicar troca o foco.

Como corolário: esconder uma tela continua sendo `hidden`, e nunca `opacity: 0`,
`visibility: hidden` ou mover para fora da viewport. Só o `hidden` faz o `adaptiveStream`
soltar a camada; os outros três deixam a assinatura viva com a imagem invisível — que é o
pior dos dois mundos.

## Consequências

**Fica fácil:** manter o custo de egress previsível e proporcional ao que está na tela;
explicar por que o produto não tem a faixa de miniaturas do Discord.

**Fica caro:** trocar de tela no modo foco é reconhecer um avatar em vez de reconhecer uma
imagem. É pior, e é o preço.

**Fica proibido:** faixa de miniaturas ao vivo, grade de pré-visualização no seletor de
foco, "espiar" a tela de alguém ao passar o mouse sobre o nome, e qualquer galeria de
telas que não seja a grade principal.

**Não vale** para o preview da própria tela ([ADR-0030](0030-preview-da-propria-tela-e-local.md)):
ele não assina nada, sai da captura local. Nem para as miniaturas do seletor de fontes, que
saem do `DesktopCapturer` da própria máquina e existem só enquanto o seletor está aberto.

## Alternativas rejeitadas

**Faixa de miniaturas assinando a camada baixa.** É o que o Discord faz, e o Discord tem
uma frota de SFUs. Nós temos uma VM, e 3,93 GB/h medidos com dois espectadores.

**Faixa de miniaturas com assinatura pausada, retomada no hover.** O tempo entre pedir a
camada e ter imagem é de segundos, não de milissegundos: o resultado seria uma faixa de
retângulos pretos que às vezes acende. Pior que avatar, e mais caro.

**Um quadro estático por tela, atualizado a cada N segundos.** O SFU não entrega quadro
avulso; entregar exigiria o publicador enviar uma track de miniatura própria, ou seja,
mais uma codificação na máquina de quem compartilha para servir a navegação de quem
assiste. Custo no lugar errado.
