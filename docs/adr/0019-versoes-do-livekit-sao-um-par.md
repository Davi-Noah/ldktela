# ADR-0019 — Cliente e servidor do LiveKit são um par fixado, atualizados juntos

- **Status:** Aceito
- **Data:** 2026-09-14

## Contexto

Durante a primeira tentativa real de compartilhar tela entre duas máquinas, o
produto falhou de um jeito que consumiu uma sessão inteira de depuração, e a
causa não era nada do que parecia ser.

O sintoma era este: o segundo cliente pareava, entrava na sala, aparecia na
interface — e nunca via a tela. Quem compartilhava via `Espectadores 0` e um
bitrate que oscilava entre 0 e 3,7 Mb/s. O log do servidor ficava inteiro verde:
`/auth/pair` 200, gateway 101, `/rooms/:id/token` 200, webhook `room_started`
chegando. Nada acusava erro.

Foram investigadas e **descartadas** cinco hipóteses, cada uma plausível:

1. **`use_external_ip` anunciando o IP público** — era um bug real e foi
   corrigido, mas não era esta causa.
2. **`LIVEKIT_URL=127.0.0.1` entregue ao cliente** — também real, também
   corrigido, também não era esta causa.
3. **CSP do Tauri sem a origem da LAN** — real no notebook, corrigido.
4. **WebKitGTK sem WebRTC** — real no Linux, registrado em `DECISIONS.md` e
   corrigido, e ainda assim o problema persistiu. Foi o que provou que a causa
   era comum às duas plataformas: o Windows Sandbox, com WebView2 e WebRTC
   nativo, falhou exatamente igual.
5. **Colisão de identidade no LiveKit** — descartada: o banco mostrou duas contas
   distintas, com sessões distintas.

O que provou a causa foi reproduzir a falha **fora do aplicativo**: um cliente
`livekit-client` mínimo, num Edge headless, contra o mesmo SFU. Ele conectava em
1,2 s e assinava sem problema, mas ao **publicar** travava exatamente 15,46 s e
reconectava. O log interno da biblioteca dizia:

```
[error] NegotiationError: negotiation timed out
[warn]  Initial connection failed: v1 RTC path not found.
        Consider upgrading your LiveKit server version
        connected to Livekit Server version: 1.8.4, protocol: 15
```

E o servidor, do outro lado, a cada tentativa:

```
WARN unsupported datachannel added {"transport": "PUBLISHER", "label": "_data_track"}
```

O cliente instalado era o **2.22.1**, falando protocolo **17**, e criava um canal
de dados (`_data_track`) que o servidor **1.8.4**, falando protocolo **15**, não
reconhece. Sem esse canal a negociação SDP da publicação nunca fecha, expira nos
15 s do `peerConnectionTimeout` do `livekit-client`, e o cliente reconecta em
laço — para sempre.

A origem do desencontro estava em duas linhas escritas em arquivos diferentes:

| Arquivo | Declarado | Instalado |
|---|---|---|
| `desktop/package.json` | `"livekit-client": "^2.7.0"` | **2.22.1** |
| `docker/compose.dev.yml` | `livekit/livekit-server:v1.8` | 1.8.4 |

Um lado com acento circunflexo, livre para derivar a cada `npm install`; o outro
fixado. Quando o projeto foi escrito, 2.7.0 e 1.8 eram contemporâneos. Quinze
versões menores depois, deixaram de ser — sem que uma linha de código mudasse,
sem aviso de compilação, e sem que `just check` tivesse como perceber.

## Decisão

**As versões do `livekit-client` e do `livekit-server` são um par, fixadas de
forma exata, e sobem juntas.**

O par vigente, verificado fim a fim, é:

| Componente | Versão |
|---|---|
| `livekit-client` (`desktop/package.json`) | `2.22.1` — exata, sem `^` |
| `livekit/livekit-server` (`docker/compose.dev.yml`) | `v1.13.6` — exata, sem tag móvel |

Atualizar um dos dois **obriga** a atualizar o outro e a repetir a verificação de
publicação descrita em `docs/DESTRAVAR.md`. Não é aceitável subir o cliente
porque uma auditoria de dependências pediu, sem tocar no servidor.

O mesmo vale para a VM de produção quando a fatia S2 existir: a versão do
servidor lá é a mesma deste arquivo, não "a mais recente".

## Consequências

**O que melhora.** A falha de publicação deixa de ser possível por deriva
silenciosa. O par está escrito, com data e número de versão, e dois testes
quebram se qualquer um dos lados voltar a ser uma faixa móvel — cada um no crate
que é dono do arquivo, para não precisar de dependência nova:

| Lado | Teste |
|---|---|
| `livekit-client` | `desktop/src/media/versions.test.ts` |
| imagem do servidor | `the_dev_compose_pins_an_exact_livekit_server_version`, em `crates/api/src/livekit.rs` |

**O que custa.** Atualizações de segurança do `livekit-client` deixam de ser
automáticas. Esse custo é aceito de propósito: é exatamente o automatismo que
quebrou o produto, e o caminho de mídia é a única coisa que o produto faz.

**O que este ADR não resolve.** O teste garante que os dois estão fixados, não
que são **compatíveis** — isso nenhum teste unitário alcança, porque exige um SFU
de verdade e uma publicação de verdade. A verificação continua sendo manual e
continua sendo obrigatória ao trocar qualquer um dos dois lados.

**Lição de método, que vale além deste caso.** O caminho de mídia falhou em
silêncio enquanto todo o plano de controle respondia 200. Nenhuma quantidade de
log no servidor teria revelado a causa, porque o servidor nunca soube que havia
um problema. O que resolveu foi reproduzir a falha num cliente mínimo, fora do
aplicativo, e ler o log da biblioteca. Isso reforça, do lado mais caro, o que o
[ADR-0018](0018-construir-antes-de-medir.md) já dizia: enquanto o caminho de
mídia não for exercitado de ponta a ponta, ele não está funcionando — está apenas
compilando.
