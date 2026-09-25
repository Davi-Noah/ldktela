# ADR-0036 — Chamadas privadas 1:1 coexistem com canais do Discord

- **Status:** Aceito
- **Data:** 2026-09-24
- **Complementa:** [ADR-0008](0008-complemento-ao-discord.md),
  [ADR-0009](0009-identidade-por-pareamento.md),
  [ADR-0010](0010-autorizacao-derivada-do-discord.md) e
  [ADR-0011](0011-sala-e-o-canal-de-voz.md)

## Contexto

O produto só abre uma sala quando o usuário está num canal de voz de um servidor do
Discord. Isso atende comunidades, mas não atende duas pessoas que já estão numa chamada
direta do Discord e querem usar apenas o plano de mídia de tela deste projeto. Uma chamada
direta não fornece ao bot um canal de voz de guild que possa virar sala do LiveKit.

O pipeline caro já existe: captura nativa, áudio, VP9, publicação, assinatura, preview e
controles do espectador. O que falta é um segundo mecanismo de identidade, admissão e ciclo
de vida da sala. Reimplementar voz, texto, câmera ou contatos continuaria fora de escopo.

## Decisão

Mantemos o modo atual de servidor sem alterações de produto: `/tela`, pareamento pelo bot,
canal de voz como sala e autorização derivada das permissões do Discord.

Adicionamos um modo independente de guild chamado **chamada privada**, com escopo fechado:

1. uma pessoa autenticada cria a chamada e recebe um código aleatório de uso único;
2. uma segunda pessoa autenticada consome o código e entra;
3. ambas podem publicar e assistir telas usando o pipeline LiveKit existente;
4. quem criou pode encerrar a chamada, o que remove os dois participantes do LiveKit e
   impede a emissão de novos tokens.

A chamada comporta exatamente o dono e o primeiro convidado. Não existe ação separada de
expulsar: numa conversa 1:1, encerrar a chamada cobre esse caso. O MVP não oferece link,
lista pública, busca, histórico de chamadas nem novos convites para a mesma chamada.

Para o modo privado, a identidade vem de OAuth2 do Discord com apenas o escopo `identify`.
O backend troca o código OAuth, lê `/users/@me`, cria ou atualiza o mesmo usuário local e
emite os tokens próprios já usados pelo aplicativo. Tokens OAuth do Discord não são
persistidos depois dessa troca. O pareamento por `/tela` continua existindo para o modo de
servidor.

A autorização da chamada privada é local e estreita: o usuário precisa ser o `owner_id` ou
o `guest_id` da chamada ativa. Essa é uma exceção explícita ao ADR-0010; salas originadas de
canais continuam usando exclusivamente a réplica e as permissões do Discord.

## Consequências

- O aplicativo ganha uma tela inicial quando não há canal de voz ativo: criar chamada ou
  entrar com código.
- O protocolo passa a distinguir sala de canal e chamada privada sem tornar os campos do
  Discord opcionais e ambíguos.
- O LiveKit precisa reconhecer nomes `dvc-<snowflake>` e `private-<uuid>`; captura,
  codificação e reprodução não mudam.
- O banco ganha tentativas efêmeras de login OAuth e uma tabela específica de chamadas
  privadas. O limite de duas pessoas fica estrutural, sem tabela genérica de membros.
- Encerrar precisa revogar admissão e desconectar participantes já conectados; expirar só o
  token LiveKit não basta.
- O operador precisa configurar `DISCORD_OAUTH_CLIENT_ID`,
  `DISCORD_OAUTH_CLIENT_SECRET` e a URL de callback.
- O serviço continua independente e sem afiliação com a Discord Inc.; OAuth fornece apenas
  identidade, não acesso a mensagens, chamadas ou mídia do Discord.

## Alternativas rejeitadas

- **Remover `/tela` e migrar tudo para OAuth.** Quebra o fluxo já funcional de comunidades
  e perde a entrada automática dirigida pelo canal de voz.
- **Usar código sem autenticar o convidado.** Transforma o código em credencial de sessão e
  impede revogação, auditoria mínima e vinculação segura do token LiveKit.
- **Criar uma sala genérica com N participantes.** Introduz moderação, convites renováveis,
  lista de membros e administração que não pertencem ao MVP 1:1.
- **Adicionar expulsão individual.** Com somente duas pessoas, ela duplica o efeito de
  encerrar e cria estado adicional sem entregar outro caso de uso.
- **WebRTC ponto a ponto.** Exige um segundo pipeline de sinalização, ICE, TURN e reconexão;
  o SFU existente já atende o caso com menos código novo.
