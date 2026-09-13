# ADR-0008 — O produto é complemento ao Discord, não substituto

- **Status:** Aceito
- **Data:** 2026-09-12
- **Substitui:** o escopo inteiro do SRS v1.2

## Contexto

A v1 era uma plataforma privada de comunicação que substituiria um servidor do Discord
para uma comunidade de 10 a 30 pessoas: chat, DMs, busca, voz, vídeo, tela, migração do
histórico e ponte bidirecional permanente.

Duas constatações mudaram a direção.

A primeira é de produto: o problema real que motiva o projeto não é "queremos sair do
Discord", é **compartilhamento de tela indisponível** — em regiões onde o screen share do
Discord é bloqueado por lei ou degradado a ponto de inutilidade. Todo o resto do Discord
continua funcionando e continua sendo onde a comunidade vive. Reconstruir chat, busca e
DMs é reconstruir o que não está quebrado.

A segunda é de estado do repositório, levantada em 2026-09-12: o backend está maduro
(~19.800 linhas, 342 testes verdes, estágios E0–E11a), mas **o frontend não existe** —
`desktop/src/App.tsx` renderiza um parágrafo, e `features/`, `gateway/` e `store/` são
diretórios vazios. O `spike/` de screen share, que o SRS §9 declarava ser a fatia de maior
risco técnico e mandava executar antes de tudo, **nunca foi feito**: não há um único
número medido de bitrate, egress ou latência glass-to-glass no repositório.

Ou seja: o que foi construído é a parte de baixo risco e alta reusabilidade
(autenticação, gateway WebSocket com resume, guards de permissão, emissão de token e
webhooks do LiveKit). O que não foi construído é a parte cara de um clone de Discord — a
UI inteira. O pivô acontece perto do momento de menor custo possível.

## Decisão

O produto é **um complemento ao Discord que faz exclusivamente compartilhamento de tela**.

O Discord permanece como camada social: identidade, comunidade, texto, voz, presença,
permissões. Nós somos o plano de mídia para a tela, e nada além disso.

Regra derivada, aplicável a toda proposta futura de recurso: **se o Discord já faz, não
reimplemente — integre.**

### Entra no escopo

Publicação de tela em 1080p60 com áudio, visualização por múltiplos espectadores,
identidade e autorização derivadas do Discord, sala atrelada ao canal de voz do Discord,
anúncio da sessão dentro do Discord, e resiliência de transporte.

### Sai do escopo, definitivamente

Mensagens de texto, anexos, busca, conversas diretas, reações, menções, estado de
não-lidas, cargos e permissões próprios, convites, migração de histórico, ponte
bidirecional de mensagens, microfone e câmera.

## Consequências

- O custo de manutenção do produto cai para a superfície que ninguém mais resolve por nós.
- A distribuição fica resolvida: o produto é descoberto dentro do Discord, onde os
  usuários já estão ([ADR-0011](0011-sala-e-o-canal-de-voz.md)).
- Em troca, ganhamos uma **dependência de runtime do Discord**: se o bot cai, ninguém
  entra em sala nova. Tratado no [ADR-0010](0010-autorizacao-derivada-do-discord.md).
- Um diferencial cai no colo: o Discord cobra 1080p60 via Nitro. Como a comunidade
  hospeda o próprio SFU, aqui isso é o padrão, sem assinatura.
- **Descarte assumido:** rotas de mensagens, DMs, busca e anexos, seus repositórios e
  ~2.200 linhas de teste. Os crates `bridge` e `migrator` (esqueletos de uma linha) são
  removidos inteiros. Detalhamento no [ADR-0016](0016-poda-por-reescrita-de-migrations.md).
- **Nota sobre a premissa legal:** a legalidade de operar o serviço numa dada jurisdição é
  decisão do operador que hospeda a instância, não deste projeto. O que a arquitetura faz
  a respeito é (a) não centralizar: cada comunidade hospeda a própria instância, e (b) não
  manter registro central de quem assistiu o quê, além do necessário para o orçamento de
  egress. A resiliência de transporte do [ADR-0013](0013-turn-tls-443-primario.md) é
  engenharia padrão de WebRTC — é o mesmo caminho que todo produto de mídia usa para
  atravessar CGNAT e firewall corporativo — e não é desenhada para outra finalidade.

## Alternativas rejeitadas

- **Terminar a v1 como planejada.** O gargalo real do projeto sempre foi o screen share, e
  ele continuaria intacto no fim de um clone inteiro de Discord.
- **Ferramenta de screen share genérica, sem Discord.** Perde a identidade, as permissões
  e a distribuição de graça, e passa a competir com Google Meet e Parsec num terreno onde
  não temos vantagem. O acoplamento ao Discord é a proposta de valor, não um detalhe.
- **Manter chat como "recurso de apoio" ao compartilhamento.** É a porta de entrada para
  reconstruir o Discord inteiro por acidente. Coordenação textual acontece no Discord, que
  está aberto na tela ao lado.
