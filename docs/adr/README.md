# Registro de Decisões de Arquitetura (ADR)

Este diretório é a **memória de decisões do projeto**. Existe porque o trabalho acontece
em sessões separadas, com contexto que não sobrevive entre elas: sem registro, uma
decisão já resolvida é re-litigada ou silenciosamente contrariada depois.

## Regra

**Toda decisão que restrinja trabalho futuro vira um ADR, antes de a tarefa ser dada
como pronta.** Isso inclui escolha de stack, formato de wire, modelo de autorização,
ordem do roadmap, e — principalmente — **o que fica fora de escopo**. Decisão de escopo
não registrada é a que mais custa: seis meses depois ninguém lembra se a ausência de um
recurso foi deliberada ou esquecimento.

## Onde cada coisa mora

| Tipo de registro | Onde | Exemplo |
|---|---|---|
| Decisão que molda o sistema ou é cara de reverter | `docs/adr/NNNN-titulo.md` | "autorização vem do Discord, não de RBAC próprio" |
| Nota de implementação: por que **esta linha** é assim | `docs/DECISIONS.md` | "`argon2` fixado em 0.5 porque a 0.6 reescreveu a API" |
| Requisito, schema, critério de aceite | `docs/SRS-v2.0-complemento-screen-share.md` | RF-xx, RNF-xx |

Na dúvida entre ADR e `DECISIONS.md`: se um agente ou pessoa em outra sessão poderia
razoavelmente tomar o caminho oposto e quebrar algo, é ADR.

## Formato

```markdown
# ADR-NNNN — Título afirmativo

- **Status:** Proposto | Aceito | Substituído por ADR-XXXX | Aposentado
- **Data:** AAAA-MM-DD

## Contexto
O que era verdade quando a decisão foi tomada, e qual pressão a forçou.

## Decisão
Uma afirmação no imperativo. Não "consideramos usar X", e sim "usamos X".

## Consequências
O que passa a ser fácil, o que passa a ser caro, e o que fica proibido.

## Alternativas rejeitadas
Cada uma com o motivo real da rejeição — este é o campo que evita re-litígio.
```

`Status` nunca é apagado. Uma decisão revertida vira `Substituído por ADR-XXXX`, e o ADR
novo diz o que mudou no mundo. O histórico de decisões erradas vale tanto quanto o das
certas.

## Índice

| ADR | Título | Status |
|---|---|---|
| [0001](0001-midia-no-webview.md) | Mídia no LiveKit JS SDK dentro do WebView2 | Aceito, refinado pelo 0014 |
| [0002](0002-sfu-auto-hospedado.md) | SFU LiveKit auto-hospedado na mesma VM | Aceito |
| [0003](0003-postgres-na-mesma-vm.md) | PostgreSQL auto-hospedado na mesma VM | Aceito |
| [0004](0004-uuidv7-na-aplicacao.md) | IDs UUIDv7 gerados na aplicação | Aceito |
| [0005](0005-modulo-unico-de-midia.md) | Aquisição de mídia num único módulo | Aceito |
| [0006](0006-anexos-em-tabela-propria.md) | Anexos em tabela própria | Aposentado |
| [0007](0007-governanca-de-decisoes.md) | Decisões registradas como ADR numerado | Aceito |
| [0008](0008-complemento-ao-discord.md) | O produto é complemento ao Discord, não substituto | Aceito |
| [0009](0009-identidade-por-pareamento.md) | Identidade por pareamento via bot do Discord | Aceito |
| [0010](0010-autorizacao-derivada-do-discord.md) | Autorização derivada do Discord; RBAC próprio aposentado | Aceito |
| [0011](0011-sala-e-o-canal-de-voz.md) | A sala é o canal de voz do Discord | Aceito |
| [0012](0012-midia-unidirecional.md) | Mídia unidirecional: sem microfone, câmera ou texto | Aceito |
| [0013](0013-turn-tls-443-primario.md) | TURN/TLS em 443 é caminho primário, não fallback | **Rebaixado pelo 0020** |
| [0014](0014-audio-por-aplicativo.md) | Áudio por aplicativo via WASAPI, transportado por IPC | Aceito, refinado pelo 0025; o IPC caiu no 0026 |
| [0015](0015-postgres-com-schema-reduzido.md) | Postgres mantido apesar do schema reduzido | Aceito |
| [0016](0016-poda-por-reescrita-de-migrations.md) | Poda do escopo por reescrita das migrations | Aceito |
| [0017](0017-nao-usar-discord-activity.md) | O produto não é uma Discord Activity | Aceito |
| [0018](0018-construir-antes-de-medir.md) | Construir S1 e S3–S6 antes de medir S0 | Aceito, dívida quitada em parte |
| [0019](0019-versoes-do-livekit-sao-um-par.md) | Cliente e servidor do LiveKit são um par fixado | Aceito |
| [0020](0020-o-bloqueio-e-do-discord-nao-da-rede.md) | O bloqueio é do Discord, não da rede | Aceito |
| [0021](0021-seletor-de-tela-e-o-do-chromium.md) | O seletor de tela é o do Chromium, até depois do S9 | Substituído pelo 0026 |
| [0022](0022-destacar-tela-usa-document-pip.md) | Destacar uma tela usa Document Picture-in-Picture | Aceito |
| [0023](0023-quem-publica-escolhe-resolucao-e-fps.md) | Quem publica escolhe resolução e fps | Aceito |
| [0024](0024-tag-live-no-apelido.md) | A tag `[LIVE]` vai no apelido, com guardas | Aceito |
| [0025](0025-audio-exclui-o-discord.md) | O áudio do sistema exclui a árvore do Discord | Aceito |
| [0026](0026-publicacao-no-rust-nativo.md) | A publicação vai para o core Rust; o WebView só assiste | Aceito |
| [0027](0027-publicador-e-um-segundo-participante.md) | Quem publica entra na sala como um segundo participante | Aceito |
| [0028](0028-silenciar-telas-alheias-ao-transmitir-audio.md) | Quem transmite áudio não ouve o áudio das outras telas | Aceito |
