# Roadmap — fatias verticais

Substitui o roadmap F0–F8 do SRS v1.2 §9, aposentado pelo
[ADR-0008](adr/0008-complemento-ao-discord.md).

Cada fatia entrega valor observável e tem critério de aceite executável. A numeração é
**S** (de *screen*) para não colidir com as fatias F da v1, que aparecem em commits e
documentos antigos.

## Estado (2026-09-13)

| Fatia | Estado |
|---|---|
| S0 — Spike de viabilidade | **NÃO FEITA.** Ver [ADR-0018](adr/0018-construir-antes-de-medir.md) |
| S1 — Poda | Feita |
| S2 — Transporte em produção | Não feita |
| S3 — Identidade por pareamento | Backend e bot feitos; cofre do Windows feito |
| S4 — Réplica e autorização | Feita, com revogação ao vivo |
| S5 — Sala atrelada ao canal de voz | Feita |
| S6 — Compartilhar e assistir | Cliente escrito; **não exercitado contra mídia real** |
| S7 — Cliente completo | Bandeja feita; estatísticas e qualidade feitas; atualização automática não |
| S8 — Presença no Discord | Não feita |
| S9 — Áudio por aplicativo | Não feita |

Números: backend ~5.000 linhas com 111 testes, cliente ~2.000 linhas com 36
testes, `just check` verde. Verificado em execução: o servidor sobe, aplica
migrations, responde, e sobrevive à queda do bot falhando fechado.

> **A dívida que define o estado real.** Continua sem existir **um único número
> medido** de bitrate, egress, latência glass-to-glass ou CPU. A premissa do
> [ADR-0013](adr/0013-turn-tls-443-primario.md) — a de que nossa mídia atravessa
> uma rede que bloqueia a do Discord — **nunca foi testada**. Enquanto isso não
> for feito, o produto compila e roda, mas não está provado.

## Ordem e seu motivo

O SRS v1.2 mandava executar o spike de screen share antes de tudo, porque era o
maior risco técnico. Isso não foi feito, e o projeto construiu onze estágios de
plano de controle sem nunca ter verificado se o produto é viável.

Este roadmap foi escrito para corrigir a ordem — e em 2026-09-13 ela foi
quebrada de novo, desta vez de propósito e por escrito
([ADR-0018](adr/0018-construir-antes-de-medir.md)): o produto foi construído
antes de ser medido, porque medir exige duas máquinas físicas e uma rede
hostil, e nada disso um agente executa.

A obrigação de medir não foi cancelada; mudou de alvo. O critério de aceite de
S0 passou para S6, sem abrandamento, e agora vale contra o cliente real em vez
de contra código descartável.

---

## S0 — Medição de viabilidade: transporte, qualidade e custo

**Bloqueia declarar qualquer coisa pronta.** Deixou de ser código descartável em `spike/`
e passou a ser medição contra o cliente real, que já existe
([ADR-0018](adr/0018-construir-antes-de-medir.md)). Responde de uma vez às duas perguntas
que podem matar o produto.

**Aceite** — todos os itens medidos e registrados em `docs/RESULTS.md`:

1. Sessão de screen share 1080p60 entre duas máquinas Windows contra o SFU
   auto-hospedado, estável por 30 minutos.
2. **A mesma sessão, com todo o UDP de saída bloqueado por firewall no cliente**,
   estabelecendo e sustentando por 20 minutos via TURN/TLS em 443
   ([ADR-0013](adr/0013-turn-tls-443-primario.md)).
3. Registrados, para os dois caminhos: bitrate real, latência glass-to-glass, CPU do
   compartilhador, CPU da VM e egress extrapolado por hora e por espectador.

**Ponto de decisão explícito.** Se o caminho relayado não sustentar 1080p, a resposta não
é remendar: é decidir, com o número na mão, entre baixar o alvo (1080p30, 720p60) ou
concluir que o produto não atende o mercado que motivou o pivô. Essa decisão vira ADR.

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

Torna permanente o que S0 provou: IP público dedicado para o TURN, TLS próprio, Caddy,
cloud-init versionado, faixa UDP de produção.

**Aceite:** implantação do zero reproduzível a partir do repositório; o teste de UDP
bloqueado de S0 passa contra a instância de produção; `RNF-13` (migrar para VPS
equivalente em ≤ 2 h) exercitado ao menos uma vez.

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

## S7 — Cliente completo

Seletor de qualidade, tela cheia, estatísticas para quem compartilha (bitrate, fps,
espectadores), lista de espectadores, bandeja do sistema, notificação nativa de "fulano
começou a compartilhar", atualização automática por `.msi`.

**Aceite:** o aplicativo roda o dia todo na bandeja consumindo menos de 1% de CPU e 150 MB
em repouso; a notificação não dispara para quem já está assistindo.

## S8 — Presença dentro do Discord

O bot anuncia a sessão no canal de texto associado, **editando uma única mensagem** em vez
de publicar várias, com contagem de espectadores e link profundo que abre o aplicativo
direto na sala.

**Aceite:** uma sessão inteira, do início ao fim, produz exatamente uma mensagem no
Discord; o limite de taxa de edição é respeitado sob uma sessão com entradas e saídas
frequentes; o link profundo abre o aplicativo instalado e, se não houver, a página de
download.

## S9 — Áudio por aplicativo

Captura WASAPI por processo no core Rust, PCM por IPC, injeção como track no WebView
([ADR-0014](adr/0014-audio-por-aplicativo.md)). Zona de revisão humana obrigatória.

**Aceite:** compartilhar um jogo com o Discord aberto e em uso não retransmite a voz dos
outros participantes; deriva de relógio não acumula desvio audível em 30 min; o fallback
para áudio do sistema funciona e avisa.

---

## O que deliberadamente não está aqui

Chat, anexos, busca, DMs, microfone, câmera, gravação de sessão, controle remoto,
anotação sobre a tela, clientes móveis ou web, e link de compartilhamento avulso sem
Discord. Cada um desses tem uma rejeição registrada em `docs/adr/`; se algum voltar à
mesa, o caminho é um ADR novo que substitua o anterior, não uma issue.
