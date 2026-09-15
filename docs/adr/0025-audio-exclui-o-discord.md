# ADR-0025 — O áudio do sistema exclui a árvore de processos do Discord

- **Status:** Aceito
- **Data:** 2026-09-14
- **Refina:** [ADR-0014](0014-audio-por-aplicativo.md)

## Contexto

O [ADR-0014](0014-audio-por-aplicativo.md) descreveu o problema certo — capturar
áudio do sistema devolve a voz do Discord de todos para eles — e propôs a solução
pela ponta errada: capturar **apenas** o aplicativo compartilhado, via WASAPI
process loopback em modo *include*.

Modo include obriga a escolher um processo. Um jogo com launcher, um navegador com
processo por aba, um emulador que toca som em processo separado: em todos, "o
aplicativo" não é um processo só, e o usuário teria de acertar qual.

O Windows oferece o inverso. `AUDIOCLIENT_ACTIVATION_PARAMS` aceita
`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`: capturar **todo o áudio do
sistema exceto** uma árvore de processos. Apontando para o Discord, é literalmente
o requisito do S9.

O pedido original era mais fino ainda — excluir só a voz dos participantes e
manter os outros sons do Discord. Isso não é separável: o Discord mistura voz,
notificação e mídia na mesma sessão de áudio, e o sistema operacional entrega o
mix do processo. Não há camada onde essa distinção exista.

## Decisão

A captura de áudio do sistema é feita no core Rust com
`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`, apontando para a árvore de
processos do Discord. Tudo o mais que a máquina toca entra; o Discord não.

O PCM atravessa o IPC até o WebView e é publicado como track, exatamente como o
ADR-0014 já previa — a mudança é de *modo*, não de arquitetura, e o argumento de
volume continua valendo (~384 KB/s contra os ~41 MB/s que proíbem vídeo no IPC).

Descobrir o processo do Discord é por nome de imagem (`Discord.exe` e as variantes
PTB/Canary), com o PID resolvido na hora de iniciar a captura. Não havendo Discord
rodando, não há nada a excluir e a captura é do sistema inteiro.

## Consequências

- O usuário não escolhe processo nenhum. Compartilha a tela, e o áudio que sai é
  "tudo menos o Discord" — que é o que ele queria dizer o tempo todo.
- Funciona com qualquer aplicativo multi-processo sem configuração.
- **Os sons do Discord somem da transmissão**, inclusive notificação e o som de
  entrar na chamada. Ninguém quer esses na transmissão; fica registrado como
  escolha, não como acidente.
- A exclusão é por árvore de processos: se o usuário rodar o Discord no navegador,
  em vez do aplicativo, a exclusão não o alcança e o eco volta. Caso conhecido,
  sem solução no nível do SO, e que precisa de aviso na interface.
- Continua em zona de revisão humana (`CLAUDE.md` §10): é captura de mídia com
  API de sistema operacional.

## Alternativas rejeitadas

- **Modo *include* apontado para o aplicativo compartilhado** (o ADR-0014
  original). Exige acertar o processo certo e falha em tudo que é multi-processo.
- **Excluir só a voz dos participantes.** Preferido pelo dono do projeto e não
  implementável: o Discord não separa voz de outros sons na saída do processo.
- **Pedir fone de ouvido e capturar tudo.** É o que o ADR-0012 registra como
  insuficiente: a captura de loopback é digital e pega o mix do sistema
  independentemente do dispositivo de saída. Fone não conserta.
