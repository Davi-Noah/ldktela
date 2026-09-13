# ADR-0010 — Autorização derivada do Discord; RBAC próprio aposentado

- **Status:** Aceito
- **Data:** 2026-09-12
- **Aposenta:** o algoritmo de resolução de permissão do SRS v1.2 §5.3

## Contexto

A v1 implementou RBAC completo (estágio E5): máscara de 63 bits, cargos por guild com
posição hierárquica, `@everyone` implícito, overwrites por canal para cargo e para membro,
e o algoritmo normativo de oito passos em `crates/domain/src/resolve.rs`, com testes.

É código correto que resolve o problema errado. Ele administra permissões num lugar onde
a comunidade não administra nada: quem decide quem entra no canal de voz é o servidor do
Discord, com os cargos que a comunidade já mantém há anos. Manter um segundo conjunto de
cargos garante apenas que os dois vão divergir.

## Decisão

**O Discord é a autoridade sobre quem pode ver e entrar em uma sala.** Não temos cargos,
não temos overwrites, não temos tela de administração.

O bot mantém uma **réplica local** do estado relevante (guilds, canais de voz, cargos,
membros e seus cargos), alimentada pelo gateway do Discord: estado completo no
`GUILD_CREATE` da conexão, e daí em diante por eventos incrementais. A decisão de
autorização é computada localmente contra essa réplica, aplicando o algoritmo de
permissão do próprio Discord, com as constantes vindas do tipo `Permissions` do serenity
— nunca com valores de bit escritos à mão.

Autorizar alguém a assistir a uma sala é responder a uma pergunta só: *este usuário tem
`VIEW_CHANNEL` e `CONNECT` neste canal de voz do Discord?*

## Consequências

- Somem `crates/domain/src/resolve.rs`, `crates/db/src/repo/{roles,permissions,categories}.rs`,
  as tabelas `roles`, `member_roles`, `channel_overwrites`, `categories`, `guild_members`
  e as rotas de administração correspondentes.
- **A regra do `CLAUDE.md` §2.7 é reescrita, não abandonada.** "Nunca confie em cache de
  permissão" vira: a réplica é verificada **na entrada e continuamente**. Quando um evento
  do gateway remove o acesso de alguém (saiu do servidor, perdeu o cargo, o canal virou
  privado), o backend **expulsa o espectador da sala do LiveKit imediatamente**, sem
  esperar a próxima renovação de token. Sessão de mídia é longa; permissão verificada só
  na porta é permissão que vaza por horas.
- **Intents:** exige `GUILD_MEMBERS` (privilegiado, mas dispensa aprovação abaixo de 100
  servidores) e os estados de voz. **`MESSAGE_CONTENT` deixa de ser necessário** — era o
  intent mais difícil de justificar na v1, e sai junto com a ponte de mensagens.
- **Dependência de runtime.** Se o gateway do Discord cai, a réplica envelhece. Modo
  degradado normativo: sessões em curso continuam; entradas novas são recusadas quando a
  réplica está defasada além de um limiar. Falha fechada, nunca aberta.
- O bit `SCREEN_SHARE` (17) da máscara própria some. Quem pode publicar é decidido pela
  política do produto ([ADR-0012](0012-midia-unidirecional.md)), não por um cargo nosso.

## Alternativas rejeitadas

- **Consultar a API REST do Discord a cada verificação.** Rate limit e latência no caminho
  quente, e uma indisponibilidade do Discord viraria indisponibilidade total. A réplica
  por gateway é o padrão de qualquer bot sério.
- **Manter o RBAC próprio e sincronizar com o Discord.** Duas fontes de verdade, e a
  reconciliação entre elas é o tipo de trabalho que nunca termina.
- **Simplificar para "dono da sala + lista de convidados"** (considerado antes do
  reposicionamento). Ficou obsoleto: o Discord entrega hierarquia de cargos e canais
  privados prontos, e reimplementar uma versão pobre disso seria retrocesso.
