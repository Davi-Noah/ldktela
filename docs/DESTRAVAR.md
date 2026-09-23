# Como destravar o projeto

Guia operacional, em ordem de dependência.

> **Estado em 2026-09-14.** As Fases 0, 1 e 2 estão concluídas: o produto pareia,
> entra na sala sozinho, compartilha, e os números da Fase 2 passam em RNF-02,
> RNF-03, RNF-04 e RNF-05 ([`RESULTS.md`](RESULTS.md)). A Fase 3 **foi rebaixada**
> e não decide mais nada sobre a existência do produto
> ([ADR-0020](adr/0020-transporte-comum-sem-adversario-de-rede.md)).
>
> O que fazer a seguir está em ["O teste que passou a ser o mais valioso"](#o-teste-que-passou-a-ser-o-mais-valioso).
> As fases abaixo ficam como referência de instalação e de diagnóstico.

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
- **Bot Permissions:** `View Channels`, `Send Messages`, `Manage Nicknames`

Copie a URL gerada, abra no navegador, escolha o servidor, autorize.

> `View Channels` é o mínimo para enxergar os canais de voz. `Send Messages` é o
> anúncio da sessão (S8), que o bot **edita** em vez de republicar. `Manage
> Nicknames` é a tag `[LIVE]` (RF-38).

**Depois de autorizar, mova o cargo do bot para cima dos cargos de membro**
(Configurações do servidor → Cargos, arraste o cargo do bot para perto do topo).

> Sem isso a tag `[LIVE]` simplesmente não aparece para quem tem cargo acima do
> bot — o Discord recusa, e o produto pula em silêncio com um aviso no log em vez
> de tentar e falhar a cada transmissão.
>
> **O dono do servidor nunca recebe a tag, e não há o que fazer.** Não é bug e
> não é permissão faltando: o Discord não deixa bot nenhum renomear o dono, por
> mais alto que esteja o cargo ([ADR-0024](adr/0024-tag-live-no-apelido.md)).
> Se você é o dono e quer ver a tag funcionando, teste com outra conta.

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
ROOM_MAX_PUBLISHERS=10
ROOM_MAX_CAMERAS=10
```

---

## Fase 1 — Primeira execução, uma máquina (~30 min)

Prova que identidade, autorização e sala funcionam ponta a ponta. Ainda não
prova nada sobre mídia.

### 1.0 Onde o bot encosta no aplicativo

Vale entender antes de rodar, porque explica quase toda falha desta fase.

**O bot e a API são o mesmo processo.** `just dev` roda `cargo run -p server`, e
esse binário sobe as duas coisas ao mesmo tempo, compartilhando o mesmo estado em
memória (a réplica do Discord e o registro de sessões WebSocket):

```
Discord  --gateway-->  [ bot | API + WebSocket ]  <--WS/HTTP--  aplicativo
                        um processo só, porta 8080
```

**O bot nunca fala direto com o aplicativo.** Ele escreve no estado compartilhado
e no Postgres; o aplicativo lê pela API e pelo WebSocket. Os três momentos em que
isso acontece:

| Momento | O que corre |
|---|---|
| Você digita `/tela` | Discord entrega a interação ao bot → o bot grava o **hash** do código no Postgres e responde efêmero. **O aplicativo não participa.** Só depois, quando você digita o código nele, o app chama `POST /auth/pair` e troca o código por uma sessão |
| Você entra num canal de voz | Discord manda o estado de voz ao bot → o bot resolve a permissão contra a réplica → publica `ROOM_JOIN` no hub → o aplicativo recebe pelo WebSocket e entra na sala |
| Você perde acesso no Discord | Evento do gateway → o bot varre a sala → expulsa do LiveKit e publica `ROOM_LEAVE` |

A consequência prática: **se o servidor não está rodando, o `/tela` não existe.**
O comando é registrado pelo bot ao conectar; sem processo, não há comando, e o
Discord não mostra nada nem dá erro.

### 1.1 Subir

```bash
just infra-up      # Postgres + LiveKit
just dev           # aplica migrations e sobe o servidor
```

`just dev` usa `cargo-watch` se ele existir e, se não existir, roda sem recarga
automática e avisa. Para ter recarga ao salvar: `just install-tools`.

No log você deve ver, nesta ordem:

```
listening
discord connected        bot=<nome>  guilds=1
guild espelhado          members=N  voice_channels=N  roles=N
/tela registrado         guild=<id>
```

**Leia a linha `guild espelhado`.** É o diagnóstico mais útil do arranque:

- `members` menor que `expected` → o `GUILD_CREATE` veio incompleto, e o servidor
  busca o resto pela API REST, logando `membros carregados`. Se em vez disso
  aparecer um erro citando 403, aí sim **o SERVER MEMBERS INTENT está desligado**
  (0.2) — a API REST de membros exige o mesmo intent.
- `voice_channels: 0` → o bot não enxerga canal de voz nenhum. Falta `View
  Channels`, ou os canais têm overwrite negando para o cargo do bot.

Se aparecer `Sent invalid authentication`, o token está errado — volte ao 0.4.

O comando é registrado **por servidor**, não globalmente, e por isso aparece na
hora. Se fosse global, o Discord levaria até uma hora para propagar e você
digitaria `/tela` sem ver nada.

Em outro terminal:

```bash
just app           # abre o aplicativo Tauri
```

> **A primeira vez que você roda `just app` depois desta mudança leva vários
> minutos e não parece estar funcionando.** O core passou a publicar mídia pelo
> SDK Rust do LiveKit ([ADR-0026](adr/0026-publicacao-no-rust-nativo.md)), que
> baixa ~114 MB de libwebrtc pré-compilado e compila C++ antes de chegar ao nosso
> código. Três coisas para saber antes:
>
> - **Deixe alguns GB livres.** Os artefatos de debug do libwebrtc são grandes, e
>   quando o disco enche o erro que aparece é `os error 112` no meio de um link,
>   sem dizer que é espaço.
> - **Feche o VS Code, ou pelo menos não deixe o rust-analyzer analisando o
>   projeto, durante esse primeiro build.** Ele roda `cargo check` no mesmo
>   `target/`, e os dois disputam a extração do libwebrtc. O sintoma é
>   `Failed to move extracted WebRTC into place — Acesso negado`. Se acontecer,
>   apague `desktop/src-tauri/target/debug/build/scratch-*` e rode de novo.
> - Se aparecer `C1083: cannot open include file` citando cabeçalhos que
>   claramente existem, o caminho do projeto está fundo demais: os cabeçalhos do
>   libwebrtc ficam a ~250 caracteres dentro de `target/`, e o `cl.exe` estoura o
>   `MAX_PATH` de 260. Mova o repositório para mais perto da raiz do disco.

### O roteiro de aceite

1. **Parear** — no Discord, num canal de texto do servidor, digite `/tela`. O bot
   responde só para você com um código de 8 caracteres. Digite no aplicativo.
   → Espere: o app sai da tela de pareamento.
2. **Entrar na sala sem clicar em nada** — entre num **canal de voz** do
   servidor.
   → Espere: o app troca sozinho para a tela da sala, com o nome do canal. Isto é
   o [ADR-0011](adr/0011-sala-e-o-canal-de-voz.md) funcionando; se você teve que
   escolher alguma coisa, é bug.
3. **Compartilhar** — clique em compartilhar.
   → Espere: **o nosso seletor**, listando suas telas e janelas com os nomes
   certos. Não pode aparecer a caixa do Chromium, nem a barra "você está
   compartilhando sua tela" — se qualquer uma das duas surgir, o WebView ainda
   está publicando e a migração do
   [ADR-0026](adr/0026-publicacao-no-rust-nativo.md) não pegou.
   → Espere: o painel de estatísticas mostra bitrate e fps subindo, e diz se o
   encoder é `hardware` ou `software`.
3b. **Áudio sem o Discord** — com o Discord aberto e alguém falando, compartilhe
   marcando *Incluir o áudio do sistema*.
   → Espere: o painel diz `Com áudio, sem o Discord`. Quem assiste ouve o jogo e
   **não** ouve a própria voz de volta. Se disser `Com áudio do sistema inteiro`,
   o core não achou o processo do Discord — é o fallback declarado do RF-30, não
   um silêncio.
3c. **Fechar a janela compartilhada** — compartilhe uma janela e feche-a.
   → Espere: o compartilhamento termina sozinho e o botão volta a "Compartilhar
   tela". Uma imagem congelada no lugar disso é bug.
3d. **O Discord percebe** (S8) — com alguém transmitindo, olhe o chat do canal
   de voz e a lista de membros.
   → Espere: **uma** mensagem dizendo quem transmite, em qual canal e quantos
   assistem — e ela é **editada** conforme gente entra e sai, não repetida. Ao
   parar, vira "A transmissão terminou.". Uma sessão inteira produz uma mensagem
   só.
   → Espere: `[LIVE] ` no apelido de quem transmite, sumindo ao parar. Se não
   aparecer, o log do servidor diz por quê — e "dono do servidor" é o motivo mais
   provável.
3e. **A queda não suja apelido alheio** (RF-40) — com alguém marcado, mate o
   servidor (Ctrl+C) e suba de novo.
   → Espere: no arranque, `limpando tags [LIVE] de uma queda`, e o apelido volta
   ao que era **antes** — inclusive voltando a não ter apelido, se não tinha.
3f. **Notificação nativa** (RF-27) — com o aplicativo minimizado na bandeja
   (não em foco), peça para outra pessoa compartilhar no seu canal.
   → Espere: uma notificação do Windows dizendo quem começou a transmitir. Se
   você mesmo compartilhar, ou se a janela estiver em foco, não deve aparecer
   nenhuma — é silêncio deliberado, não bug.
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
| `just dev` morre com `no such command: watch` | `cargo-watch` não instalado. Já não é fatal — atualize o repositório, ou rode `just serve` |
| **`/tela` não aparece, e nada acontece ao digitar** | **O servidor não está rodando.** É o caso mais comum: o bot registra o comando ao conectar, então sem processo não há comando. Confira se `just dev` está de pé e mostrou `/tela registrado` |
| `/tela` some depois de ter aparecido | O bot foi removido do servidor, ou perdeu o scope `applications.commands`. Refaça o 0.3 |
| **Dois `/tela` idênticos na lista** | Sobra de uma versão que registrava o comando globalmente. O servidor agora apaga os globais ao conectar — reinicie e recarregue o Discord (Ctrl+R) |
| **"Servidor indisponível. Tente de novo." ao colar o código** | O fetch nem saiu da máquina. Era falta de CORS no servidor (corrigido). Se voltar: o WebView é sempre uma origem diferente da API, então a origem precisa estar em `ALLOWED_ORIGINS` (`crates/api/src/lib.rs`) **e** no `connect-src` do `tauri.conf.json` |
| App em branco, nenhum erro de rede | CSP do Tauri não cobre a origem. Veja `desktop/src-tauri/tauri.conf.json` → `connect-src` |
| Pareia, mas entrar em sala dá 404 | Réplica sem membros → **SERVER MEMBERS INTENT** desligado (0.2). O log de arranque diz `members: 1` e emite um `WARN` |
| **Compartilha, o bitrate oscila, e ninguém vê. Tudo responde 200** | Versões do LiveKit fora do par ([ADR-0019](adr/0019-versoes-do-livekit-sao-um-par.md)). Confirme com `docker logs ldktela-livekit \| grep "unsupported datachannel"`: se aparecer, o cliente fala um protocolo que o servidor não entende, a negociação de **publicação** expira em 15 s e o cliente reconecta em laço. Conectar e assinar continuam funcionando, e é por isso que o log fica todo verde |
| Compartilha, mas ninguém vê | Webhook do LiveKit não chega ao backend. Em Linux confira `extra_hosts` no `docker/compose.dev.yml` |
| Servidor recusa subir | Falta variável no `.env`. A mensagem nomeia qual |
| **O app está pareado na conta errada** | Bandeja → **Trocar de conta**. Isso revoga a sessão no servidor, limpa o cofre e volta ao pareamento. O aplicativo não tem como descobrir sozinho qual conta do Discord está aberta na máquina |
| **Dois computadores com o mesmo pareamento** | Só um funciona: o LiveKit expulsa a identidade repetida, e antes disso os dois trocavam a sala em laço. Hoje o segundo para e avisa. Cada máquina precisa do seu próprio `/tela` |
| **Build do app: `Failed to move extracted WebRTC into place — Acesso negado`** | Dois cargos no mesmo `target/`, quase sempre o rust-analyzer do VS Code. Feche-o, apague `desktop/src-tauri/target/debug/build/scratch-*` e rode de novo |
| **Build do app: `os error 112` no meio de um link** | Disco cheio. O libwebrtc em debug ocupa vários GB |
| **Build do app: `C1083` citando cabeçalho que existe** | `MAX_PATH`. Repositório fundo demais; mova para perto da raiz |
| **Build do app: centenas de `LNK2038 RuntimeLibrary`** | O `crt-static` não foi aplicado. O cargo lê `.cargo/config.toml` pelo diretório **atual**: rode de dentro de `desktop/src-tauri`, nunca com `--manifest-path` de fora |
| **Compartilha, mas a outra pessoa não aparece na lista de espectadores** | Conexão de publicação contada como pessoa. Ela usa a identidade `<uuid>~pub` ([ADR-0027](adr/0027-publicador-e-um-segundo-participante.md)) e deve ser ignorada em presença |
| **O áudio sai com a voz do Discord junto** | O painel do publicador diz `Com áudio do sistema inteiro`: o core não achou o Discord rodando ao iniciar. Pare e recomece o compartilhamento com o Discord aberto |
| **Compartilhando com áudio, não ouço a tela do outro** | É deliberado ([ADR-0028](adr/0028-silenciar-telas-alheias-ao-transmitir-audio.md)): o nosso próprio som entra na nossa captura e voltaria para a sala |

### Verificar a publicação ao trocar a versão do LiveKit

Obrigatório sempre que `livekit-client` ou `livekit/livekit-server` mudar
([ADR-0019](adr/0019-versoes-do-livekit-sao-um-par.md)). O teste automatizado
garante que os dois estão fixados, **não** que são compatíveis — isso exige um
SFU de verdade e uma publicação de verdade.

Com `just dev` de pé, compartilhe a tela e observe, nesta ordem:

1. No painel do app, o bitrate **sobe e permanece**. Se ele oscila entre 0 e um
   valor alto num ciclo regular, a negociação está expirando.
2. `docker logs ldktela-livekit | grep -c "unsupported datachannel"` responde
   **0**. Qualquer número acima disso é desencontro de protocolo.
3. `docker logs ldktela-livekit | grep "participant closing"` **não** mostra um
   `CLIENT_REQUEST_LEAVE` a cada 15 s.

O terceiro é o mais confiável: 15 segundos cravados e repetidos é a assinatura do
`peerConnectionTimeout` do `livekit-client`, não de um problema de rede.

> **Limite conhecido:** o Discord manda a lista de membros no `GUILD_CREATE` só
> até o `large_threshold` (50 por padrão). Em servidores maiores a réplica nasce
> incompleta e membros ausentes recebem 404. Para os 10–30 usuários do perfil de
> uso (SRS §1.3) não morde; num servidor grande, morde. Não está resolvido.

---

## Fase 2 — Duas máquinas, caminho direto

Aqui saem os primeiros números reais. Mede o **melhor caso** (UDP direto): é o
piso do que o produto consegue, não a condição em que ele precisa funcionar.

### 2.1 Preparar a rede

O LiveKit de desenvolvimento anuncia `127.0.0.1`, que a outra máquina não
alcança. Em `docker/livekit.dev.yaml`, dentro de `rtc:`, troque **só o valor de
`node_ip`** pelo IP da máquina na LAN:

```yaml
  use_external_ip: false   # NÃO mexa nesta linha
  node_ip: 192.168.x.y     # o IP da MÁQUINA na LAN, não do container
```

> **`use_external_ip` tem que continuar `false`.** Com `true`, o LiveKit
> descobre o IP **público** por STUN e ignora o `node_ip`: os candidatos ICE
> saem com o IP do provedor, que nem a própria máquina alcança, porque roteador
> doméstico não faz hairpin NAT. O sintoma é `ConnectionError: could not
> establish pc connection` com a sinalização funcionando normalmente — token
> emitido, webhook `room_started` chegando, e a mídia falhando em silêncio.
> Para confirmar, `docker logs ldktela-livekit | head` e leia o `nodeIP` da
> linha `starting LiveKit server`: tem que ser o IP da LAN.

Suba de novo (`just infra-down && just infra-up`). A faixa UDP 50000–50019 já
está publicada no compose — sem ela a mídia cairia no fallback TCP e você mediria
o caminho errado.

Libere no firewall do Windows, na máquina servidora: TCP 8080, TCP 7880, TCP 7881
e UDP 50000–50019.

No `.env` da raiz, troque também o `LIVEKIT_URL`:

```
LIVEKIT_URL=ws://192.168.x.y:7880
```

> **Este é o segundo endereço que precisa sair do `localhost`, e é fácil
> esquecer** porque ele não aparece em nenhuma config do cliente. O servidor
> **entrega esse valor ao aplicativo** na resposta de `POST /rooms/:id/token`, e
> o cliente conecta no que recebeu. Com `127.0.0.1`, a segunda máquina tenta
> conectar no próprio localhost: ela entra na sala pelo WebSocket, aparece na
> interface, e nunca chega ao SFU — não vê ninguém e ninguém a vê. Reinicie o
> servidor depois de mudar.

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
| **CPU do compartilhador** | PowerShell: `Get-Process ldktela* \| Measure-Object WorkingSet64,CPU -Sum`. Some a árvore de processos, como manda o RNF-03 |
| **CPU e RAM da máquina do SFU** | `docker stats ldktela-livekit` |
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

## Fase 3 — Caminho relayado (rebaixada)

**Deixou de ser o teste que decide o produto.** Em 2026-09-14 a premissa que
sustentava esse status caiu: não há bloqueio de rede a mídia em tempo real na
região alvo — quem desligou o compartilhamento de tela foi o próprio Discord, por
motivos dele ([ADR-0020](adr/0020-transporte-comum-sem-adversario-de-rede.md)).

O que resta desta fase é a verificação normal de CGNAT: **quem estiver atrás de
NAT restritivo só conecta pelo relay.** Isso não é sobre a existência do produto,
é sobre uma parte dos usuários conseguir usá-lo. Roda junto de S2, quando houver
TURN.

### 3.1 Pré-requisito: TURN (fatia S2, escopo reduzido)

Sem IP dedicado e sem disputar a 443 com o Caddy — isso só se justificava pela premissa
de um adversário de rede, que não existe (ADR-0020). Uma VM comum com Caddy, LiveKit e
TURN na porta padrão resolve. A 443 continua sendo a porta que mais atravessa
firewall corporativo; se sair barata, vale.

### 3.2 O teste

Na máquina cliente, bloqueie **todo o UDP de saída**, preservando DNS. Num
PowerShell como administrador:

```powershell
# A regra de permissão vem primeiro: no Windows Firewall, allow vence block.
netsh advfirewall firewall add rule name="ldktela-teste-dns" dir=out action=allow protocol=UDP remoteport=53
netsh advfirewall firewall add rule name="ldktela-teste-bloqueio-udp" dir=out action=block protocol=UDP
```

Com as regras ativas, faça uma sessão 1080p e confirme que ela estabelece e se
sustenta, com **100% das conexões relayadas** — se não forem 100%, o bloqueio não
pegou. Registre os mesmos números da 2.3 em `docs/RESULTS.md`.

Para remover:

```powershell
netsh advfirewall firewall delete rule name="ldktela-teste-bloqueio-udp"
netsh advfirewall firewall delete rule name="ldktela-teste-dns"
```

---

## O teste que passou a ser o mais valioso

Não é a Fase 3. É **repetir a medição de qualidade com o espectador em outra
máquina física**, com decode acelerado por hardware.

A Fase 2 registrou 139 s de congelamento em 26 min e concluiu, corretamente, que
o número é **inconclusivo**: as três pontas dividiam a mesma CPU, e a assinatura
— buffer de jitter subindo, RTT saltando, perda quase nula — é de contenção, não
de rede. Enquanto isso não for refeito, não se sabe se o produto engasga.

É barato, e é o que separa "funciona na minha máquina" de "funciona".

## Lacunas conhecidas, para você não descobrir sozinho

Coisas que estão faltando de propósito ou que eu não consegui fechar:

1. **Egress não é medido.** `share_sessions.egress_bytes` e
   `sessions::add_egress` existem, mas nada os chama: o webhook do LiveKit não
   carrega bytes. O RNF-05 exige medição real, e hoje só há extrapolação. Fechar
   isso significa ler as métricas do LiveKit periodicamente.
2. ~~Réplica incompleta acima de ~50 membros~~ — **resolvido**: quando o
   `GUILD_CREATE` vem truncado, o bot busca a lista completa pela API REST,
   paginando. O `large_threshold` deixou de importar.
3. **Sem testes de integração HTTP no `api`.** A suíte antiga testava rotas que
   não existem mais e foi removida; os repositórios têm testes contra Postgres
   real, mas as rotas novas (`/auth/pair`, `/rooms/*`, webhook) só têm cobertura
   por unidade.
4. **`docs/rest-api.md` e `docs/websocket.md` descrevem o contrato antigo**, com
   aviso no topo dizendo o que sobrevive. A reescrita não foi feita.
5. **Link profundo no anúncio do Discord não existe** — o Discord não torna
   esquema próprio clicável, então falta a página https de redirecionamento que
   o destrava. Ver [ADR-0029](adr/0029-link-profundo-espera-uma-pagina-https.md).
6. ~~Atualização automática (RF-28), anúncio no Discord (S8) e áudio por
   aplicativo (S9)~~ — **feitos**. Ver a seção seguinte para publicar a primeira
   versão assinada.

## Publicar uma versão (RF-28)

O aplicativo verifica atualização sozinho — 10 s depois de abrir e depois a
cada 6 h — contra as Releases do repositório, e só instala um pacote cuja
assinatura bate com a chave pública embutida em `tauri.conf.json`. Faltam dois
segredos, que só existem depois de alguém gerar o par de chaves:

```bash
cd desktop && npm run tauri signer generate -- -w $HOME/.tauri/ldktela.key
```

Isso imprime a chave pública (já foi colada em `tauri.conf.json`) e grava a
privada em `~/.tauri/ldktela.key`. Nos **segredos do repositório** no GitHub
(Settings → Secrets and variables → Actions), cadastre:

- `TAURI_SIGNING_PRIVATE_KEY` — o conteúdo do arquivo `ldktela.key`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — a senha escolhida ao gerar.

**A chave privada nunca deve existir fora desses dois lugares** (o arquivo
local e o segredo do GitHub): quem a tiver pode publicar uma atualização para
todo mundo que usa o aplicativo.

Com os segredos cadastrados, publicar é criar uma tag:

```bash
git tag v0.2.0 && git push origin v0.2.0
```

O workflow `.github/workflows/release.yml` compila, assina e sobe o `.msi` e o
`latest.json` como um **rascunho** de release — publique-o manualmente na aba
Releases quando estiver pronto para que os clientes existentes o vejam.

---

## Resumo do caminho crítico

```
Fase 0  (feita)      token do bot
Fase 1  (feita)      roteiro de aceite
Fase 2  (feita)      medicao real -> RNF-02/03/04/05 passam (RESULTS.md)
  |
  +-- PROXIMO: qualidade com espectador em outra maquina  <- maior valor, barato
  |
  +-- S2: VM com TURN (escopo reduzido)                   <- precisa de voce
  |     +-- Fase 3: caminho relayado, junto de S2
  |
  +-- S9: audio por aplicativo                            <- maior risco de produto
```

O produto funciona ponta a ponta e cabe no orçamento. O que falta para chamá-lo
de pronto é uma medição limpa de qualidade, uma implantação de verdade, e o
áudio.
