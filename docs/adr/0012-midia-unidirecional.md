# ADR-0012 — Mídia unidirecional: sem microfone, câmera ou texto

- **Status:** Aceito, **exceto quanto à câmera** — rejeição substituída pelo
  [ADR-0038](0038-camera-e-uma-segunda-publicacao.md) em 2026-09-22. Sem microfone, sem
  texto e sem publicação por quem assiste continua valendo.
- **Data:** 2026-09-12

## Contexto

A v1 previa voz, câmera e tela na mesma sala, com push-to-talk global (RF-33), simulcast
de câmera com teto de 3 publicadores (RF-21) e supressão de ruído (RF-23). Isso implica
topologia bidirecional: todo participante publica e assina ao mesmo tempo.

Com o [ADR-0008](0008-complemento-ao-discord.md), a voz continua no Discord — onde
funciona, onde as pessoas já têm suas configurações de microfone, atalhos e volume por
usuário ajustados.

## Decisão

A topologia de mídia é **um publicador, N assinantes, sem retorno**.

Quem compartilha publica exatamente uma track de vídeo (a tela) e, no máximo, uma track de
áudio (o áudio do que está sendo compartilhado). Quem assiste **não publica nada**. Não há
microfone, não há câmera, não há chat de texto, não há reações.

O teto de publicadores simultâneos por sala é configurável, com padrão 2 (suficiente para
comparar duas telas), reaproveitando o ledger de admissão em memória já implementado como
guard de câmera em `crates/api/src/voice.rs`.

## Consequências

- A matemática de egress fica simples e previsível: uma tela para N espectadores.
- O token do LiveKit emitido para espectador não recebe permissão de publicação alguma —
  o `can_publish_sources` restrito que já existe fica ainda mais estreito.
- Somem push-to-talk global, atalho de teclado para microfone, supressão de ruído,
  indicador de "falando", simulcast de câmera e o próprio conceito de mudo.
- **Um problema novo aparece, e é sério.** Como a voz está no Discord e a captura de áudio
  do Chromium só existe em modo "áudio do sistema", a captura do compartilhador inclui a
  saída do Discord — ou seja, a voz de todos os outros — e a devolve para eles com atraso.
  Fone de ouvido **não** resolve: a captura de loopback é digital e pega o mix do sistema
  independentemente de onde ele é reproduzido. O aviso da v1 (RF-22c) tratava de
  realimentação por microfone, que é outro problema. Este é tratado no
  [ADR-0014](0014-audio-por-aplicativo.md), e até ele existir a mitigação é
  compartilhar sem áudio ou aceitar o eco conscientemente.

## Alternativas rejeitadas

- **Carregar a voz também.** Duplica o egress, reimplementa o que o Discord faz bem, e
  obriga o usuário a reconfigurar microfone, atalhos e volumes num segundo aplicativo.
- **Câmera junto da tela.** Discord já faz, e cada publicador adicional multiplica o
  egress que é o único recurso escasso do produto.
  > **Revertido em 2026-09-22 pelo [ADR-0038](0038-camera-e-uma-segunda-publicacao.md).** O
  > "Discord já faz" não se sustentou: a câmera dele tem a mesma limitação de qualidade que
  > o compartilhamento de tela. O argumento do egress sobreviveu, e virou teto por fonte.
- **Chat mínimo "só para coordenar".** O Discord está aberto na tela ao lado. Um chat
  nosso seria um segundo lugar onde procurar a mesma conversa.
