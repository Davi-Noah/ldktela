# Roadmap — fatias verticais

Substitui o roadmap F0–F8 do SRS v1.2 §9, aposentado pelo
[ADR-0008](adr/0008-complemento-ao-discord.md).

Cada fatia entrega valor observável e tem critério de aceite executável. A numeração é
**S** (de *screen*) para não colidir com as fatias F da v1, que aparecem em commits e
documentos antigos.

## Estado (2026-09-14)

| Fatia | Estado |
|---|---|
| S0 — Medição de viabilidade | **Fase 2 feita e aprovada** (`RESULTS.md`). Fase 3 rebaixada — ver abaixo |
| S1 — Poda | Feita |
| S2 — Transporte em produção | Não feita. Escopo reduzido pelo [ADR-0020](adr/0020-o-bloqueio-e-do-discord-nao-da-rede.md) |
| S3 — Identidade por pareamento | Feita |
| S4 — Réplica e autorização | Feita, com revogação ao vivo |
| S5 — Sala atrelada ao canal de voz | Feita |
| S6 — Compartilhar e assistir | **Funciona ponta a ponta**, medido em 2026-09-14 |
| S7 — Cliente completo | Várias telas, grade e foco, destacar, volume e tempo no ar feitos. Seletor próprio e publicação nativa feitos ([ADR-0026](adr/0026-publicacao-no-rust-nativo.md)). Notificação nativa feita. Atualização por `.msi` pendente de chave de assinatura |
| S8 — Presença no Discord | Anúncio (uma mensagem por sessão, editada) e tag `[LIVE]` com as quatro guardas, feitos. Link profundo adiado: o Discord não torna esquema próprio clicável ([ADR-0029](adr/0029-link-profundo-espera-uma-pagina-https.md)) |
| S9 — Áudio por aplicativo | Feita e medida: modo `ExcludingDiscord`, 143.520 amostras/canal em 3 s. Falta ouvir numa sessão real entre duas máquinas |

### O que mudou em 2026-09-14

A premissa que travava o projeto caiu, e não por medição: **não há bloqueio de
rede a mídia em tempo real na região alvo.** Quem desligou o compartilhamento de
tela foi o próprio Discord. O [ADR-0013](adr/0013-turn-tls-443-primario.md) foi
rebaixado pelo [ADR-0020](adr/0020-o-bloqueio-e-do-discord-nao-da-rede.md): o
TURN continua no roadmap por CGNAT, que é engenharia comum, e não como condição
de existência do produto.

A Fase 2 mediu o que faltava e passou: latência p95 de 212 ms, 585 MB no
publicador, ~1 núcleo de encode, 3,93 GB/h de egress com 2 espectadores. Detalhe
e método em [`RESULTS.md`](RESULTS.md).

> **Os dois riscos que sobraram**, em ordem:
>
> 1. **Congelamentos.** 139 s em 26 min, que o `RESULTS.md` registra como
>    **inconclusivo** — as três pontas dividiam a mesma máquina e a assinatura é
>    de contenção de CPU, não de rede. Precisa de uma medição limpa com o
>    espectador em outra máquina antes de virar aprovação ou defeito.
> 2. **Áudio.** O caso de uso central é assistir gameplay, e hoje compartilhar
>    com áudio devolve a voz do Discord de todos para eles
>    ([ADR-0014](adr/0014-audio-por-aplicativo.md), fatia S9).
>
> Nota sobre a folga de latência: os 212 ms foram medidos com RTT de 2–3 ms, com
> tudo na mesma máquina. Contra uma VM real, o RTT some ~40 ms e o p95 vai para a
> casa dos 250 ms. Continua passando o RNF-02, com ~15% de folga em vez de 29%.

## Como destravar

O passo a passo operacional — token do bot, roteiro de aceite, e como executar a
medição de S0 — está em [`DESTRAVAR.md`](DESTRAVAR.md).

## Ordem e seu motivo

O SRS v1.2 mandava executar o spike de screen share antes de tudo, porque era o
maior risco técnico. Isso não foi feito, e o projeto construiu onze estágios de
plano de controle sem nunca ter verificado se o produto era viável.

Este roadmap foi escrito para corrigir a ordem, e em 2026-09-13 ela foi quebrada
de novo, de propósito e por escrito
([ADR-0018](adr/0018-construir-antes-de-medir.md)). Em 2026-09-14 a dívida foi
paga: a medição saiu, contra o cliente real, e aprovou.

O que resta medir não é mais um portão do projeto — é a verificação do caminho
relayado, que acontece junto de S2.

---

## S0 — Medição de viabilidade: transporte, qualidade e custo

**Fase 2 concluída em 2026-09-14 e aprovada.** Números e método em
[`RESULTS.md`](RESULTS.md): RNF-02, RNF-03, RNF-04 e RNF-05 passam.

Pendente, e agora sem status de portão:

1. **Repetir a medição de qualidade com o espectador em outra máquina**, com
   decode acelerado. Os 139 s de congelamento da Fase 2 são inconclusivos porque
   as três pontas dividiam a mesma CPU. Este é o item de maior valor do roadmap
   hoje, e é barato.
2. **Caminho relayado**, junto de S2: uma sessão 1080p com o UDP de saída
   bloqueado no cliente, para confirmar que quem está atrás de CGNAT conecta.
   Deixou de decidir se o produto existe
   ([ADR-0020](adr/0020-o-bloqueio-e-do-discord-nao-da-rede.md)); decide se uma
   parte dos usuários consegue usar.

## S1 — Poda do escopo

**Feita em 2026-09-13**, sob o aval de
[ADR-0016](adr/0016-poda-por-reescrita-de-migrations.md).

Removeu rotas de mensagens, DMs, busca e anexos; os repositórios correspondentes; os
crates `bridge` e `migrator`; as dependências `argon2`, `aws-sdk-s3`, `validator`,
`marked`, `shiki`, `@tanstack/react-virtual` e `@tanstack/react-query`; e reescreveu as
migrations. Schema de 20 tabelas para 5, gateway de 30 eventos para 8, REST de 53 rotas
para 7.

**Aceite:** `just check` verde; nenhuma rota morta no router; o binário não linka mais
`argon2` nem o SDK da AWS; `docs/rest-api.md` e `docs/websocket.md` descrevem só o que
existe.

## S2 — Transporte em produção

Escopo **reduzido** pelo [ADR-0020](adr/0020-o-bloqueio-e-do-discord-nao-da-rede.md):
sem IP público dedicado, sem segundo certificado, sem disputar a 443 com o Caddy.
Uma VM comum com Caddy, LiveKit e TURN na porta padrão resolve o caso do CGNAT,
que é o que restou de motivo. A 443 continua sendo a porta que mais atravessa
firewall corporativo — se sair barata, vale; se não, 5349 serve.

Inclui cloud-init versionado, TLS, domínio, e a faixa UDP de produção.

**Aceite:** implantação do zero reproduzível a partir do repositório; uma sessão
1080p com o UDP de saída bloqueado no cliente conecta pelo relay; `RNF-12`
(migrar para VPS equivalente em ≤ 2 h) exercitado ao menos uma vez.

## S3 — Identidade por pareamento

Bot do Discord com comando de barra, código efêmero, resolução para
`discord_user_id`, emissão do par de tokens já existente
([ADR-0009](adr/0009-identidade-por-pareamento.md)). Inclui o mínimo nativo que falta:
IPC do Tauri e cofre do Windows para o refresh token.

**Aceite:** pareamento ponta a ponta contra um Discord real; código expirado, reusado ou
de outro usuário é recusado; o refresh token não aparece em disco fora do cofre; a
detecção de reúso de família continua passando.

## S4 — Réplica do Discord e autorização

Réplica local de guilds, canais de voz, cargos e membros por gateway; cálculo de
permissão contra a réplica; **revogação ao vivo**
([ADR-0010](adr/0010-autorizacao-derivada-do-discord.md)).

**Aceite:** remover o usuário do canal, do cargo ou do servidor no Discord o desconecta da
sala do LiveKit em menos de 5 s, sem esperar renovação de token. Réplica defasada além do
limiar recusa entradas novas e mantém as sessões em curso.

## S5 — Sala atrelada ao canal de voz

Estados de voz do Discord dirigem a entrada e saída da sala; nome da sala derivado do
snowflake ([ADR-0011](adr/0011-sala-e-o-canal-de-voz.md)).

**Aceite:** entrar num canal de voz do Discord faz o aplicativo entrar na sala
correspondente sem nenhum clique; sair faz o inverso; não existe seletor de sala na UI.

## S6 — Compartilhar e assistir

A primeira fatia em que o produto existe de ponta a ponta. Captura de tela com
`contentHint: 'motion'`, `degradationPreference: 'maintain-framerate'`, camadas de
simulcast, `adaptiveStream` e `dynacast`, teto de publicadores
([ADR-0012](adr/0012-midia-unidirecional.md)), e o visualizador mínimo.

**Aceite:** sessão real com 6 pessoas, uma compartilhando e cinco assistindo, por 30
minutos; egress medido dentro do previsto em S0; espectador vê o primeiro frame em menos
de 3 s.

## S7 — Cliente completo: várias telas ao mesmo tempo

A maior fatia restante. Escopo detalhado em RF-31 a RF-37 do SRS.

- **Várias telas simultâneas** (RF-31): N publicadores para N espectadores. O
  cliente deixa de assumir uma tela só.
- **Grade e foco** (RF-32): a grade assina a camada baixa, o foco pede a alta.
  É o que separa ~190 h de ~380 h no orçamento de egress
  ([ADR-0023](adr/0023-quem-publica-escolhe-resolucao-e-fps.md)).
- **Destacar em outro monitor** (RF-33) via Document Picture-in-Picture, sem
  conexão extra ([ADR-0022](adr/0022-destacar-tela-usa-document-pip.md)).
- **Dono e tempo no ar** (RF-34), com o início vindo do servidor.
- **Volume por tela** (RF-35).
- **Resolução e fps no publicador** (RF-36); no espectador, camada
  ([ADR-0023](adr/0023-quem-publica-escolhe-resolucao-e-fps.md)).
- **Interface própria de compartilhar** (RF-37), **seletor de fonte incluído**.
  Exigiu levar a publicação para o core Rust
  ([ADR-0026](adr/0026-publicacao-no-rust-nativo.md)), o que substitui o
  [ADR-0021](adr/0021-seletor-de-tela-e-o-do-chromium.md). O WebView continua
  assistindo; só deixou de publicar.
- Bandeja, notificação nativa, atualização automática por `.msi`.

**Aceite:** duas telas publicadas e vistas ao mesmo tempo por dois espectadores;
uma delas destacada em outro monitor; volume independente por tela; o tempo no ar
bate com o do servidor para quem entra depois; a grade não consome camada alta;
o seletor é o nosso e a barra do Chromium não aparece em momento algum.

## S8 — Presença dentro do Discord

O bot anuncia a sessão no canal de texto associado, **editando uma única
mensagem** em vez de publicar várias, com contagem de espectadores e link
profundo que abre o aplicativo direto na sala.

Mais a **tag `[LIVE]`** no apelido de quem transmite (RF-38 a RF-40), com as
guardas do [ADR-0024](adr/0024-tag-live-no-apelido.md): hierarquia verificada
antes de tentar, apelido anterior restaurado exatamente, limpeza no arranque.
Exige `MANAGE_NICKNAMES` e o cargo do bot acima dos cargos de membro — e **o dono
do servidor nunca recebe a tag**, que é limitação do Discord sem contorno.

**Aceite:** uma sessão inteira produz exatamente uma mensagem no Discord; o limite
de taxa de edição é respeitado sob entradas e saídas frequentes; o link profundo
abre o aplicativo instalado e, se não houver, a página de download; a tag aparece
e some junto com a transmissão; matar o processo com alguém marcado e subir de
novo deixa o apelido limpo.

## S9 — Áudio sem o Discord

Captura WASAPI no core Rust em modo
`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`, apontando para a árvore de
processos do Discord: sai todo o áudio do sistema **menos** o Discord
([ADR-0025](adr/0025-audio-exclui-o-discord.md)). Zona de revisão humana.

Com a publicação já no core ([ADR-0026](adr/0026-publicacao-no-rust-nativo.md)), o
PCM **não atravessa IPC nem `AudioContext`**: vai direto ao `NativeAudioSource` do
publicador. A deriva de relógio que o [ADR-0014](adr/0014-audio-por-aplicativo.md)
apontava como a maior incerteza do roadmap deixou de existir — captura e
codificação estão no mesmo processo e no mesmo relógio.

A exclusão aceita **um** processo, e ele é gasto no Discord; por isso quem
transmite áudio silencia as telas alheias enquanto transmite
([ADR-0028](adr/0028-silenciar-telas-alheias-ao-transmitir-audio.md)).

O compartilhamento com áudio já funciona hoje; o que falta é a exclusão. Excluir
só a voz dos participantes, mantendo os outros sons do Discord, **não é
possível** — o Discord mistura tudo num processo só.

**Aceite:** compartilhar um jogo com o Discord aberto e em uso não retransmite a
voz dos outros participantes; o áudio não dessincroniza do vídeo em 30 min; sem
Discord rodando, a captura é do sistema inteiro e nada quebra; onde o process
loopback não existir, cai para o sistema inteiro com aviso (RF-30).

---

## O que deliberadamente não está aqui

Chat, anexos, busca, DMs, microfone, câmera, gravação de sessão, controle remoto,
anotação sobre a tela, clientes móveis ou web, e link de compartilhamento avulso sem
Discord. Cada um desses tem uma rejeição registrada em `docs/adr/`; se algum voltar à
mesa, o caminho é um ADR novo que substitua o anterior, não uma issue.
