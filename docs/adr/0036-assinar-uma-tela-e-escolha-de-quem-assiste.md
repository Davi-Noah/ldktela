# ADR-0036 — Assinar uma tela é escolha de quem assiste

- **Status:** Aceito
- **Data:** 2026-09-20
- **Complementa o** [ADR-0031](0031-navegacao-nao-assina-video.md)

## Contexto

Até aqui, entrar numa sala era receber **todas** as telas que estivessem no ar. Não havia recusa:
quem entrasse num canal com três transmissões pagava as três em banda, decodificação e atenção,
mesmo querendo ver uma (issue #6).

O [ADR-0031](0031-navegacao-nao-assina-video.md) já tinha estabelecido a metade cara disso: nenhum
elemento de navegação assina vídeo. Faltava a outra metade, que é o espectador poder dizer "esta
não".

`adaptiveStream` reduz a camada de uma tela pequena ou invisível, mas não é recusa: a trilha
continua assinada, continua chegando e continua sendo decodificada.

## Decisão

**Cada tela recebida tem um estado de assinatura por espectador**, `subscribed`, que começa
ligado — o padrão continua sendo o de hoje, ver todo mundo.

Sair de uma tela é `setSubscribed(false)` na publicação do LiveKit, e não esconder o ladrilho.
Esconder não devolveria banda nenhuma, que é justamente o que se quer devolver.

**O ladrilho sobrevive à saída, vazio.** Sem trilha, mas no lugar, com o nome de quem transmite e
um botão de voltar. É a única superfície de onde se pode voltar a entrar, e apagá-la deixaria a
recusa sem caminho de volta.

**Quem parou de transmitir leva o ladrilho junto**, tendo sido assinado ou não. Isso exige ouvir
`TrackUnpublished`, e não só `TrackUnsubscribed`: para quem saiu daquela tela, o segundo evento já
aconteceu, e sem o primeiro sobraria na tela um convite para entrar numa transmissão que acabou.

## Consequências

- Quem assiste passa a poder cortar o custo do que não quer ver, que é o pedido da issue #6 e o
  caminho mais direto para a investigação de consumo de hardware da issue #12.
- O servidor não sabe de nada disso. A assinatura é uma negociação entre espectador e SFU; a
  admissão à sala, que é onde mora a autorização (ADR-0010), não muda.
- "Quem está vendo a sua tela" (issue #10) passa a ser uma aproximação: a lista conta quem está na
  sala de mídia, não quem assinou a sua trilha. Corrigir isso exigiria o servidor observar
  assinaturas por participante, e não vale o acoplamento por enquanto.
- Voltar a entrar custa o tempo de uma assinatura nova e alguns quadros até a imagem aparecer. É o
  mesmo custo de quando a tela chega pela primeira vez.
