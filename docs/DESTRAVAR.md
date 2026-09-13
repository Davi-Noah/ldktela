# Como destravar o projeto

Guia operacional. Quatro fases, em ordem de dependência. A Fase 0 e a 1 são de
hoje; a 2 dá os primeiros números; a 3 é a que decide se o produto existe.

Leia a Fase 3 antes de começar: ela muda o que você vai querer preparar na 2.

---

## Fase 0 — Bot do Discord (~15 min)

Sem isto nada de identidade ou autorização funciona. É o único segredo que o
projeto precisa e que só você pode obter.

### 0.1 Criar a aplicação

1. Abra <https://discord.com/developers/applications> e clique em **New
   Application**. Dê o nome que o usuário vai ver no Discord.
2. Menu lateral → **Bot** → **Reset Token** → copie. **O token aparece uma vez
   só.** Se perder, resete de novo.

### 0.2 Habilitar o intent privilegiado

Ainda em **Bot**, seção *Privileged Gateway Intents*:

| Intent | Estado | Por quê |
|---|---|---|
| **SERVER MEMBERS INTENT** | **LIGADO** | Sem ele a réplica nasce sem membros e **todo mundo recebe 404** ao tentar entrar numa sala |
| PRESENCE INTENT | desligado | Não usamos presença do Discord |
| MESSAGE CONTENT INTENT | **desligado** | Não lemos mensagens. Era o intent mais difícil de justificar na v1 e saiu junto com a ponte ([ADR-0010](adr/0010-autorizacao-derivada-do-discord.md)) |

Abaixo de 100 servidores o Members Intent dispensa aprovação do Discord — mas
não dispensa o clique.

### 0.3 Convidar o bot para o servidor

Menu lateral → **OAuth2** → **URL Generator**:

- **Scopes:** `bot` **e** `applications.commands`
  (sem o segundo, o comando `/tela` não aparece)
- **Bot Permissions:** `View Channels`

Copie a URL gerada, abra no navegador, escolha o servidor, autorize.

> `View Channels` é o mínimo. O bot não envia mensagem em canal — a resposta do
> `/tela` é efêmera e não precisa de permissão. Quando a fatia S8 (anúncio no
> Discord) existir, aí sim entra `Send Messages`.

### 0.4 Colar o token

No `.env` da raiz (não versionado):

```
DISCORD_BOT_TOKEN=<o token que você copiou>
```

Confira que estas cinco linhas também existem — foram adicionadas no pivô:

```
DISCORD_REPLICA_GRACE_SECONDS=60
PAIRING_CODE_TTL_SECONDS=300
PAIRING_MAX_CODES_PER_HOUR=10
ROOM_TOKEN_TTL_SECONDS=3600
ROOM_MAX_PUBLISHERS=2
```

---

## Fase 1 — Primeira execução, uma máquina (~30 min)

Prova que identidade, autorização e sala funcionam ponta a ponta. Ainda não
prova nada sobre mídia.

```bash
just infra-up      # Postgres + LiveKit
just dev           # aplica migrations e sobe o servidor
```

No log você deve ver, nesta ordem: `listening` → `discord connected`. Se aparecer
`Sent invalid authentication`, o token está errado — volte ao 0.4.

Em outro terminal:

```bash
just app           # abre o aplicativo Tauri
```

### O roteiro de aceite

1. **Parear** — no Discord, num canal de texto do servidor, digite `/tela`. O bot
   responde só para você com um código de 8 caracteres. Digite no aplicativo.
   → Espere: o app sai da tela de pareamento.
2. **Entrar na sala sem clicar em nada** — entre num **canal de voz** do
   servidor.
   → Espere: o app troca sozinho para a tela da sala, com o nome do canal. Isto é
   o [ADR-0011](adr/0011-sala-e-o-canal-de-voz.md) funcionando; se você teve que
   escolher alguma coisa, é bug.
3. **Compartilhar** — clique em compartilhar, escolha a tela inteira.
   → Espere: o painel de estatísticas mostra bitrate e fps subindo.
4. **Revogação ao vivo** — pelo Discord, tire seu próprio acesso ao canal de voz
   (um overwrite negando `Ver canal` para você, ou saia do servidor num usuário
   de teste).
   → Espere: o app é expulso da sala em menos de 5 s, com o motivo
   `access_revoked`. Este é o RF-08, e é o teste que eu mais recomendo fazer com
   atenção — é a parte do produto onde um erro é invisível e caro.
5. **Falha fechada** — pare o servidor, espere passar
   `DISCORD_REPLICA_GRACE_SECONDS`, suba de novo sem rede.
   → Espere: entrar em sala responde `503 REPLICA_STALE`, e não um "sim" chutado.

### Se algo falhar

| Sintoma | Causa provável |
|---|---|
| App em branco, nenhum erro de rede | CSP do Tauri não cobre a origem. Veja `desktop/src-tauri/tauri.conf.json` → `connect-src` |
| `/tela` não aparece no Discord | Faltou o scope `applications.commands` no convite. Refaça o 0.3 |
| Pareia, mas entrar em sala dá 404 | Réplica sem membros → **SERVER MEMBERS INTENT** desligado (0.2) |
| Compartilha, mas ninguém vê | Webhook do LiveKit não chega ao backend. Em Linux confira `extra_hosts` no `docker/compose.dev.yml` |
| Servidor recusa subir | Falta variável no `.env`. A mensagem nomeia qual |

> **Limite conhecido:** o Discord manda a lista de membros no `GUILD_CREATE` só
> até o `large_threshold` (50 por padrão). Em servidores maiores a réplica nasce
> incompleta e membros ausentes recebem 404. Para os 10–30 usuários do perfil de
> uso (SRS §1.3) não morde; num servidor grande, morde. Não está resolvido.

---

## Fase 2 — Duas máquinas, caminho direto

Aqui saem os primeiros números reais. Mede o **melhor caso** (UDP direto): é o
piso do que o produto consegue, não a condição em que ele precisa funcionar.

### 2.1 Preparar a rede

O LiveKit de desenvolvimento anuncia o IP do container, que a outra máquina não
alcança. Em `docker/livekit.dev.yaml`, dentro de `rtc:`, troque
`use_external_ip: false` por:

```yaml
  node_ip: 192.168.x.y     # o IP da MÁQUINA na LAN, não do container
```

Suba de novo (`just infra-down && just infra-up`). A faixa UDP 50000–50019 já
está publicada no compose — sem ela a mídia cairia no fallback TCP e você mediria
o caminho errado.

Libere no firewall do Windows, na máquina servidora: TCP 8080, TCP 7880, TCP 7881
e UDP 50000–50019.

### 2.2 Preparar o segundo cliente

Na segunda máquina, aponte o app para a primeira:

```bash
# desktop/.env.local
VITE_SERVER_ORIGIN=http://192.168.x.y:8080
```

E acrescente essa origem ao `connect-src` do `tauri.conf.json` (o CSP é estático;
sem isso o app não conecta e não diz por quê).

Alternativa a instalar o toolchain nas duas máquinas: `npm run tauri build` gera
um `.msi` em `desktop/src-tauri/target/release/bundle/msi/`.

### 2.3 O que medir

Compartilhe 1080p60 por **30 minutos** e registre:

| Número | Como |
|---|---|
| **Bitrate real** | Painel de estatísticas do próprio app (RF-21), amostrado a cada 2 s. Anote a mediana e o pico |
| **fps e resolução efetivos** | Mesmo painel. Se cair para 720p sob carga, o `degradationPreference` está trabalhando — anote quando |
| **CPU do compartilhador** | PowerShell: `Get-Process ldkcord* \| Measure-Object WorkingSet64,CPU -Sum`. Some a árvore de processos, como manda o RNF-03 |
| **CPU e RAM da máquina do SFU** | `docker stats ldkcord-livekit` |
| **Latência glass-to-glass** | Ver 2.4 |
| **Egress por hora** | `bitrate × nº de espectadores × 3600`. **Não** confie na coluna `share_sessions.egress_bytes`: ela existe mas ninguém a preenche ainda (ver "Lacunas") |

### 2.4 Medir glass-to-glass

O método que funciona sem instrumentação:

1. Na máquina que compartilha, abra um cronômetro com milissegundos em tela
   cheia (`https://time.is` serve, ou qualquer relógio local com ms).
2. Compartilhe essa tela.
3. Ponha os dois monitores lado a lado e **fotografe os dois numa foto só**, com
   o celular.
4. A diferença entre os dois relógios na foto é a latência glass-to-glass.
5. Repita **20 vezes**. Anote o p95, não a média — a média esconde o engasgo que
   é o que o usuário sente.

Meta do RNF-02: p95 < 300 ms. Numa LAN você deve ficar bem abaixo; o número que
importa de verdade é o da Fase 3.

---

## Fase 3 — O teste que decide o produto

**Esta é a fase que destrava o projeto de verdade.** Todo o resto é preparação.

O produto existe porque o compartilhamento de tela do Discord não funciona na
região alvo, e a causa mais provável é a rede descartando mídia em tempo real.
Nossa mídia enfrenta o mesmo adversário. Se ela também não passar, não há
produto — só um clone que falha pelo mesmo motivo
([ADR-0013](adr/0013-turn-tls-443-primario.md)).

### 3.1 Pré-requisito: TURN/TLS em 443 (fatia S2, não construída)

O teste não pode ser feito no ambiente de desenvolvimento atual: **não existe
TURN configurado**. Com UDP bloqueado, hoje a conexão simplesmente falha — e isso
não diria nada sobre o produto, só sobre a config de dev.

Antes da 3.2 é preciso:

1. Provisionar a VM com Postgres, LiveKit, Caddy e o servidor.
2. Dar ao TURN um **IP público dedicado**, com hostname e certificado próprios —
   a 443 já é do Caddy e os dois não dividem a porta.
3. Configurar `turn:` no LiveKit com `tls_port: 443` e o certificado.
4. Acrescentar o domínio de produção ao `connect-src` do `tauri.conf.json`.

Esta é a fatia S2 do roadmap. É trabalho de infraestrutura, e está em zona de
revisão humana (`CLAUDE.md` §10) — posso escrever o cloud-init e a configuração,
mas quem aplica é você.

### 3.2 O teste

Na máquina cliente, bloqueie **todo o UDP de saída**, preservando DNS. Num
PowerShell como administrador:

```powershell
# A regra de permissão vem primeiro: no Windows Firewall, allow vence block.
netsh advfirewall firewall add rule name="ldkcord-teste-dns" dir=out action=allow protocol=UDP remoteport=53
netsh advfirewall firewall add rule name="ldkcord-teste-bloqueio-udp" dir=out action=block protocol=UDP
```

Com as regras ativas, faça uma sessão 1080p de **20 minutos** e registre os
mesmos números da 2.3, com um a mais: **a proporção de conexões relayadas** —
elas devem ser 100%, ou o bloqueio não pegou.

Para remover:

```powershell
netsh advfirewall firewall delete rule name="ldkcord-teste-bloqueio-udp"
netsh advfirewall firewall delete rule name="ldkcord-teste-dns"
```

### 3.3 O ponto de decisão

Escreva os números em `docs/RESULTS.md` e decida, com eles na mão:

- **Sustentou 1080p pelo relay** → a premissa do produto está provada. Siga para
  S7, S8, S9.
- **Não sustentou** → **não remende.** Decida entre baixar o alvo (1080p30, ou
  720p60) ou concluir que o produto não atende o mercado que motivou o pivô. Essa
  decisão vira um ADR novo, seja qual for.

O erro que não pode se repetir é o da v1: onze estágios construídos sem nunca
verificar se o produto era viável.

---

## Lacunas conhecidas, para você não descobrir sozinho

Coisas que estão faltando de propósito ou que eu não consegui fechar:

1. **Egress não é medido.** `share_sessions.egress_bytes` e
   `sessions::add_egress` existem, mas nada os chama: o webhook do LiveKit não
   carrega bytes. O RNF-05 exige medição real, e hoje só há extrapolação. Fechar
   isso significa ler as métricas do LiveKit periodicamente.
2. **Réplica incompleta acima de ~50 membros** (ver Fase 1).
3. **Sem testes de integração HTTP no `api`.** A suíte antiga testava rotas que
   não existem mais e foi removida; os repositórios têm 19 testes contra Postgres
   real, mas as rotas novas (`/auth/pair`, `/rooms/*`, webhook) só têm cobertura
   por unidade.
4. **`docs/rest-api.md` e `docs/websocket.md` descrevem o contrato antigo**, com
   aviso no topo dizendo o que sobrevive. A reescrita não foi feita.
5. **Atualização automática (RF-28), anúncio no Discord (S8) e áudio por
   aplicativo (S9)** não existem.
6. **Notificação nativa (RF-27)** — o plugin está registrado, mas nada dispara
   notificação ainda.

---

## Resumo do caminho crítico

```
Fase 0 (15 min)      token do bot          -> destrava tudo
Fase 1 (30 min)      roteiro de aceite     -> prova identidade/autorizacao/sala
Fase 2 (1 dia)       duas maquinas, LAN    -> primeiros numeros, melhor caso
  |
  +-- S2: VM com TURN/TLS 443              <- trabalho de infra, precisa de voce
  |
Fase 3               UDP bloqueado         -> DECIDE SE O PRODUTO EXISTE
```

Enquanto a Fase 3 não tiver números, o produto compila, roda e não está provado.
