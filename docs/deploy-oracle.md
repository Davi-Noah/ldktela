# Implantação na VM da Oracle Cloud

Alvo: uma instância Always Free (2 OCPU, 12 GB) rodando Postgres, LiveKit e o servidor em
Docker, com o aplicativo desktop conectando de fora.

Este documento existe porque duas coisas da implantação atual precisam mudar antes de um
lançamento, e as duas exigem uma ação na VM que nenhum código do repositório pode fazer
sozinho.

---

## 1. Rotacione a chave do LiveKit. Ela está publicada.

**O servidor que está no ar usa o segredo que estava versionado em
`docker/livekit.remote.yaml`.** Qualquer pessoa com acesso ao repositório podia cunhar um
token de sala e entrar — ou publicar — em qualquer canal. Não é uma exposição hipotética: o
`.env.remote` da máquina de desenvolvimento ainda tinha exatamente o valor de exemplo.

O arquivo versionado não tem mais chave nenhuma, e o LiveKit agora recusa subir sem uma
(`one of key-file or keys must be provided`) — falhar barulhento em vez de cair num padrão.

Na VM:

```bash
# Gere um par novo com o próprio servidor, em vez de inventar um.
docker run --rm livekit/livekit-server:v1.13.6 generate-keys

# Ponha o par nas duas variáveis do .env.remote. São a ÚNICA fonte da chave:
# o compose injeta LIVEKIT_KEYS no container a partir delas.
#   LIVEKIT_API_KEY=<a chave gerada>
#   LIVEKIT_API_SECRET=<o segredo gerado>
```

O segredo antigo continua no histórico do git. Reescrever o histórico é decisão sua; rotacionar
a chave é o que tira o servidor do alcance dele, e basta.

> Enquanto estiver mexendo aí: `JWT_SIGNING_KEY` no `.env.remote` é a outra chave do sistema, e
> ela assina as sessões dos usuários. Se em algum momento ela saiu de uma máquina sua, rotacione
> junto — o custo é todo mundo parear de novo.

---

## 2. Troque a faixa UDP por uma porta multiplexada

**O que muda:** `50000-50019/udp` (vinte portas) passa a ser `7882/udp` (uma, multiplexada).

**Por que:** cada participante gasta uma porta (o core Rust, que usa conexão única) ou duas (o
WebView). Quem compartilha, portanto, custa três. Um canal de voz com sete pessoas usando o
aplicativo estoura vinte portas — e o modo de falha é o pior que existe: o ICE não conecta, a
sinalização continua funcionando, a sala aparece normal, e **simplesmente não aparece vídeo**.
É indistinguível de uma captura morta, e foi um dos candidatos para o relato de "compartilhei
do Sandbox e nada apareceu no PC".

Multiplexada, uma porta atende todas as sessões. É o arranjo que o LiveKit recomenda para
Docker, e de quebra tira vinte processos `docker-proxy` do caminho de cada pacote.

### Passos, nesta ordem

**a) Abra 7882/udp onde você abriu a faixa 50000-50019.** Foram, muito provavelmente, dois
lugares:

1. **Security List (ou NSG) da VCN**, no console da Oracle:
   *Networking → Virtual Cloud Networks → sua VCN → Security Lists → Default → Add Ingress Rule*
   - Source CIDR `0.0.0.0/0`, IP Protocol `UDP`, Destination Port Range `7882`.

2. **iptables da VM** — as imagens da Oracle vêm com uma regra `REJECT` no fim da cadeia
   `INPUT`. Confira se a faixa antiga estava lá:

   ```bash
   sudo iptables -L INPUT -n --line-numbers | grep -E "50000|7880"
   ```

   Se aparecer, acrescente a nova antes do `REJECT`:

   ```bash
   sudo iptables -I INPUT 6 -p udp --dport 7882 -j ACCEPT
   # Oracle Linux:
   sudo firewall-cmd --permanent --add-port=7882/udp && sudo firewall-cmd --reload
   # Ubuntu na OCI:
   sudo netfilter-persistent save
   ```

**b) Suba de novo:**

```bash
cd ~/ldktela     # onde o repositório está na VM
git pull
just remote-down
just remote-up
just remote-ports     # confere o que o servidor vai realmente usar
```

`just remote-ports` lê a configuração de verdade e imprime as portas. Espere ver
`7882 - ICE/UDP`. Se imprimir a faixa, o `git pull` não pegou.

**c) Só depois de confirmar que funciona, remova a regra de ingresso de 50000-50019.**

### O risco de errar esse passo é menor do que parece

Se 7882 ficar fechada, a mídia **não morre**: o ICE cai para TCP em 7881, que continua aberta e
mapeada, e a transmissão funciona — pior, mas funciona. E o painel do publicador passa a mostrar
`Transporte: tcp` com um aviso dizendo exatamente isso. Ou seja: o pior caso desta troca é
qualidade degradada e diagnosticável, não tela preta.

Detalhe que reforça a escolha: `7882` é o **padrão do próprio LiveKit** quando nada é
configurado (`rtc.portUDP {Start: 7882, End: 0}`). A faixa é que era o desvio.

### Se 7882 não puder ser aberta

Reverter é trocar duas linhas: em `docker/livekit.remote.yaml`, `udp_port: 7882` volta a ser
`port_range_start`/`port_range_end`; em `docker/compose.remote.yml`, o mapeamento de porta
acompanha. A troca de chave da seção 1 é independente e deve ficar de qualquer jeito.

---

## 3. O endereço da VM não está no repositório

O repositório é público; o endereço é de quem hospeda. Ele entra por três variáveis, todas fora
do git:

| onde | variável | o que faz |
|---|---|---|
| `.env.remote` (VM) | `LIVEKIT_NODE_IP` | o IP que o LiveKit anuncia nos candidatos ICE; o compose o passa como `NODE_IP` e recusa subir sem ele |
| `desktop/.env.production` (máquina de build) | `VITE_SERVER_ORIGIN` | para onde o cliente faz requisição |
| idem | `LIVEKIT_PUBLIC_URL` | o `ws://` do LiveKit, só para o CSP |

Copie `desktop/.env.production.example` para `desktop/.env.production` e preencha. No CI, os
mesmos dois nomes são **variáveis do repositório** (Settings → Secrets and variables → Actions →
Variables), não segredos: o endereço vai dentro do `.msi` de qualquer jeito.

O IP é fixo, e não descoberto por STUN (`use_external_ip: false`): a descoberta resolve
`stun1.l.google.com` no boot, e numa recriação o container recebeu o `127.0.0.53` do host como
DNS e entrou em loop de reinício.

### O aplicativo precisa ser recompilado quando o endereço muda

O CSP versionado em `desktop/src-tauri/tauri.conf.json` só conhece servidores de
desenvolvimento. `desktop/scripts/release-config.mjs` acrescenta o servidor público ao
`connect-src` e entrega o resultado em `TAURI_CONFIG`, que o `tauri-codegen` mescla sobre o
arquivo e embute no binário. `just build-app` e o fluxo de release já fazem isso.

`vite.config.ts` recusa a build se `VITE_SERVER_ORIGIN` faltar, e recusa se o CSP efetivo não o
permitir — porque o silêncio ali já produziu um `.msi` apontando para `http://127.0.0.1:8080`, a
máquina de quem instalasse, e porque um CSP que não alcança o servidor faz o aplicativo abrir,
pintar a interface e ter toda requisição bloqueada sem erro nenhum.

Se a porta da API mudar (`API_PORT` no `.env.remote`), as variáveis de build mudam também.

---

## 4. Verifique de verdade, contra esta VM

Nenhuma medição em `localhost` responde pela implantação: o caminho de rede que o usuário
percorre não é percorrido ali. A medição ponta a ponta aceita outro servidor por variável de
ambiente, e é ela que dá a resposta:

```bash
# Na máquina Windows de desenvolvimento, em desktop/src-tauri
LK_URL=ws://<IP_PUBLICO_VM>:7880 \
LK_KEY=<LIVEKIT_API_KEY> \
LK_SECRET=<LIVEKIT_API_SECRET> \
  cargo test --release -- --ignored --nocapture measure_1080p60
```

Ela publica uma tela real, assina do outro lado e **conta quadros no espectador**, que é a
única medição que não pode mentir. O que ela imprime a cada dois segundos:

| campo | o que esperar | o que significa se vier diferente |
|---|---|---|
| `espectador` | ~60 /s | abaixo de 50 é o defeito, não a rede |
| `resolucao` | 1920×1080 | menor: o SFU escolheu outra camada |
| `caminho` | `udp` | `tcp` é porta UDP fechada; `relay/...` é TURN |
| `limite` | `none` | `bandwidth` é a subida de quem transmite; `cpu` é a máquina |
| `banda` | acima de 6000 kb/s | abaixo disso, 1080p60 não cabe na subida |

Referência, medida em loopback num i5-10400 depois da correção do [ADR-0032](adr/0032-a-escada-do-vp9-e-temporal.md):
**59,5 quadros por segundo a 1920×1080 no espectador**, estável ao longo de 20 s, com
`limite: none`.

---

## 5. O que a VM aguenta, e o que ela não decide

O SFU **não transcodifica**: ele encaminha pacotes e escolhe camada. Duas OCPUs sobram para
isso com folga, e o gargalo de uma transmissão não está na VM.

| recurso | conta | veredito |
|---|---|---|
| CPU da VM | encaminhamento de pacotes, sem codec | folgado |
| RAM | Postgres 128 MB + LiveKit + servidor | folgado em 12 GB |
| banda | 1 Gb/s por OCPU | folgado |
| **egress do plano gratuito** | 10 TB/mês | ~2,6 GB/hora por espectador a 6 Mb/s; dá ~3.800 h de sessão com dois espectadores |

**O que decide a qualidade é a subida de quem transmite**, não a VM. 1080p60 pede algo em torno
de 6 Mb/s sustentados de upload. O painel do publicador diz qual dos dois está limitando, em
`Limitado por` — e é por isso que ele existe.

---

## 6. Antes de publicar a primeira versão

**Os dois segredos de assinatura precisam existir no GitHub** — `TAURI_SIGNING_PRIVATE_KEY` e
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, em *Settings → Secrets and variables → Actions*.

Isto não é opcional e falha mal: sem eles a build **continua**, o `.msi` sai, e o que não sai é
a assinatura. O aplicativo instala normalmente e nunca mais consegue se atualizar, porque o
atualizador só aceita um pacote assinado pela chave que está no `tauri.conf.json`. Verificado
localmente: a build imprime `A public key has been found, but no private key` e **termina com
código 0**.

A chave privada foi gerada com `npm run tauri signer generate` e só deve existir na sua máquina
e nesses dois segredos. A pública já está versionada, que é o lugar dela.

**Quem esta instância serve precisa estar decidido**, porque o instalador é público e aponta
para ela ([ADR-0035](adr/0035-a-instancia-serve-servidores-nomeados.md)). Duas camadas:

1. No Developer Portal do Discord, **desligue o *Public Bot*** (Bot → Public Bot). Sem isso,
   qualquer pessoa com permissão de gerenciar um servidor adiciona o seu bot ao dela e passa a
   usar a sua banda.
2. No `.env.remote`, liste os IDs em `DISCORD_ALLOWED_GUILDS`, separados por vírgula. Servidor
   fora da lista não é espelhado, e tudo depois disso recusa sozinho; o `/tela` de lá responde
   explicando e aponta para o repositório. Vazio serve todos.

```bash
# Com o modo de programador ligado no Discord: clique direito no servidor → Copiar ID.
DISCORD_ALLOWED_GUILDS=230754607679275010,1436472446168600698
```

Trocar a lista é editar o arquivo e `just remote-up`; não exige build nem release nova.

---

## 7. O que fica pendente, de propósito

- **Sem TLS.** `ws://` e `http://` na porta pública. Para uma primeira versão entre conhecidos
  passa; para distribuir, um domínio e um certificado entram antes. O [ADR-0020](adr/0020-transporte-comum-sem-adversario-de-rede.md)
  explica por que TURN/443 deixou de ser caminho primário, e isso não muda essa conclusão.
- **Sem TURN próprio.** Quem não conseguir UDP cai para ICE/TCP em 7881, que funciona e fica
  ruim — e o painel mostra `tcp` em `Transporte`, então dá para saber que foi isso.
- **Egress não é medido.** `sessions::add_egress` não tem chamador. O `prometheus_port: 6789`
  do LiveKit expõe os contadores reais; ligar os dois é o que fecha o RNF-05.
