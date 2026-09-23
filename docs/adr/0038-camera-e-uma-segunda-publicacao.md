# ADR-0038 — Câmera é uma segunda publicação, ao lado da tela

- **Status:** Aceito
- **Data:** 2026-09-22
- **Substitui:** a rejeição de câmera do [ADR-0012](0012-midia-unidirecional.md). O resto
  do 0012 continua valendo integralmente: sem microfone, sem texto, sem reações, e nenhum
  retorno de quem assiste.

## Contexto

O [ADR-0012](0012-midia-unidirecional.md) rejeitou câmera com dois argumentos. O primeiro
era "o Discord já faz". Ele caiu: a câmera do Discord sofre **a mesma limitação de
qualidade** que o compartilhamento de tela, que é a razão de este produto existir. Um
complemento de mídia que resolve a tela e devolve o rosto para o lugar ruim resolveu
metade do problema.

O segundo argumento — cada publicador adicional multiplica o egress, que é o único recurso
escasso — continua inteiro. Ele deixa de ser motivo de recusa e passa a ser **restrição de
projeto**: aparece como teto por fonte, e não como ausência do recurso.

## Decisão

1. **A câmera entra no escopo como uma segunda publicação de vídeo**, independente da
   tela. Uma pessoa pode transmitir a tela, a câmera, ou as duas ao mesmo tempo.

2. **O que continua fora:** microfone, texto, reações e qualquer publicação por quem
   assiste. A câmera **nunca** carrega áudio. A única trilha de áudio do produto continua
   sendo o som do que é compartilhado ([ADR-0025](0025-audio-exclui-o-discord.md)), e a voz
   continua no Discord.

3. **A unidade do domínio deixa de ser a pessoa e passa a ser a publicação**, identificada
   pelo par (pessoa, fonte), com fonte em `screen` ou `camera`. Isso vale no fio, no
   backend e no espectador: ladrilho, foco, destacar, volume, qualidade, sair-e-voltar
   ([ADR-0036](0036-assinar-uma-tela-e-escolha-de-quem-assiste.md)) e estatística passam a
   ser **por publicação**, não por pessoa.

4. **Uma conexão de publicação, duas trilhas.** O
   [ADR-0027](0027-publicador-e-um-segundo-participante.md) fica intacto: a mesma conexão
   `~pub` publica a tela e a câmera, a câmera como `Track.Source.Camera`. Uma segunda
   conexão por fonte dobraria participantes, tokens e webhooks para não resolver nada.

5. **Token:** `can_publish_sources` ganha `camera`. O token de quem assiste continua sem
   permissão de publicação alguma — a topologia segue de mão única por construção.

6. **Capacidade por sala é por fonte, configurável, e falha fechada.** As telas mantêm o
   teto que já têm; as câmeras ganham um teto próprio. Câmera é mais barata que tela, mas
   não é de graça, e um teto só para as duas deixaria quatro rostos ocuparem o lugar de
   duas telas.

7. **A qualidade da câmera é fixa: 720p a 30 fps, escada temporal `L1T3`**
   ([ADR-0032](0032-a-escada-do-vp9-e-temporal.md)). Sem 1080p e sem 60 fps, e sem o
   seletor de preset que a tela tem ([ADR-0023](0023-quem-publica-escolhe-resolucao-e-fps.md)):
   é um rosto, não texto de 9 px em movimento.

8. **A captura é nossa, em Media Foundation**, num `camera.rs` novo. O binding do libwebrtc
   que usamos traz `desktop_capturer` e **não** traz câmera, então não há caminho pronto.
   Entra na zona de revisão humana junto com o resto de `desktop/src-tauri/`.

   > Alterado pelo [ADR-0039](0039-a-camera-tem-dois-caminhos-de-captura.md): a captura
   > continua nossa, mas tem dois caminhos. O Media Foundation segue na frente; o DirectShow
   > atende as câmeras virtuais que o MF enumera e não abre.

9. **O preview da própria câmera é local** ([ADR-0030](0030-preview-da-propria-tela-e-local.md)),
   nunca pelo SFU, e é **espelhado apenas no preview**. A trilha que sai não é espelhada:
   espelhar o que os outros veem inverteria qualquer texto na frente da câmera.

10. **Paridade de HUD, sem exceção.** Tudo que a tela tem — crachá, tempo no ar, controle
    de volume quando houver áudio, qualidade, foco, destacar em popup, sair e voltar,
    miniatura no seletor, aviso na bandeja, atalho de parada, chime, notificação — a câmera
    tem. É o que "bem polida do início ao fim" quer dizer, e é o que torna esta uma fatia
    grande em vez de um botão novo.

## Consequências

- **Fio:** `SHARE_START` e `SHARE_STOP` ganham `source`. Continuam vindo só do webhook do
  LiveKit e continuam entrando no buffer de resume (`docs/websocket.md` §7).
- **Banco:** o início de cada publicação passa a morar em `room_presence`, em uma coluna
  por fonte, e é de lá que sai o tempo no ar (RF-34). `share_sessions` fica **intacta**:
  uma sessão cobre o período em que a pessoa esteve ao vivo, com qualquer fonte, porque o
  que ela serve é o orçamento de egress (RNF-05), que soma por pessoa. Isso mantém a
  migration puramente aditiva — sem drop de índice, sem alteração de tipo — e portanto fora
  da zona que exige aval humano (CLAUDE.md §10). O preço é que o histórico não distingue
  uma sessão só de câmera de uma só de tela; quando alguém precisar dessa distinção, aí sim
  será uma migration destrutiva, com a conversa que ela exige.
- **Egress:** no pior caso, dobra por pessoa. O que segura é o teto por fonte, e o número
  certo dele só existe quando a fatia S0 medir o custo real — que continua sendo a maior
  dívida do projeto.
- **Bot:** a tag `[LIVE]` ([ADR-0024](0024-tag-live-no-apelido.md)) continua uma por
  pessoa. Quem transmite qualquer coisa está ao vivo; a tag não distingue fonte.
- **Câmera ocupada** por outro aplicativo — inclusive o Discord — é caso normal no Windows,
  não erro excepcional. Precisa de mensagem que diga qual é o problema, e a tela continua
  funcionando quando a câmera falha.
- **A re-chaveação do espectador** atravessa quase toda a `desktop/src/features/room/`,
  incluindo os três layouts da v1.1. É o maior custo desta decisão, e é pago uma vez.

## Alternativas rejeitadas

- **Câmera como mais uma fonte no seletor, com uma trilha só.** Muito mais barata: nada da
  re-chaveação do item 3, e todo o HUD existente continuaria valendo sem uma linha
  alterada. Rejeitada porque impede rosto e jogo ao mesmo tempo, que é o caso de uso
  principal de uma watch party.
- **Compor a câmera sobre a tela na captura**, como um OBS embutido. Um quadro só, egress
  de um. Rejeitada porque queima o layout dentro do encode: quem assiste não pode mover,
  esconder nem focar o rosto, e a composição custa CPU exatamente de quem já está jogando.
- **Publicar a câmera pelo WebView com `getUserMedia`.** Reverteria o
  [ADR-0026](0026-publicacao-no-rust-nativo.md) e partiria a publicação entre dois motores
  de mídia, contra o [ADR-0005](0005-modulo-unico-de-midia.md), para economizar a captura
  que é justamente o trabalho que precisa ser bem feito.
- **Microfone junto, já que a câmera voltou.** Continua rejeitado pelo
  [ADR-0012](0012-midia-unidirecional.md), e por ele sozinho: a voz funciona no Discord,
  com os volumes e atalhos que cada um já ajustou.
