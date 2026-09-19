# ADR-0035 — A instância hospedada serve os servidores que ela nomeia

- **Status:** Aceito
- **Data:** 2026-09-19
- **Complementa o** [ADR-0010](0010-autorizacao-derivada-do-discord.md)

## Contexto

O repositório vai ser público, sob GPL-3.0, e o instalador vai ser publicado nas releases do
GitHub. As duas coisas juntas criam um problema que nenhuma delas tem sozinha: **o instalador
publicado aponta para a VM de quem mantém o projeto**, e qualquer pessoa pode baixá-lo.

Quem hospeda paga a banda de quem serve. Uma Always Free da Oracle com uma comunidade pequena é
uma coisa; a mesma VM servindo qualquer um que achou o link é outra. Some-se a isso que quem
hospeda responde, na prática, pelo que trafega ali.

A autorização já é derivada do Discord (ADR-0010): sem o bot num servidor não existe `/tela`,
sem `/tela` não existe código de pareamento, e sem pareamento não sai token de sala. Isso já
barra o desconhecido — **desde que o bot não possa ser adicionado por qualquer um.** O
interruptor "Public Bot" do Developer Portal decide isso, e é configuração de painel: não
aparece no código, não vai para o git, e ninguém revisa.

Restrição no aplicativo não foi considerada seriamente. O código é aberto e o binário é
editável; uma senha no cliente é removida por quem quiser em minutos. **A única fronteira que
vale é a do servidor.**

## Decisão

**A instância nomeia os servidores do Discord que ela serve**, em `DISCORD_ALLOWED_GUILDS`
(IDs separados por vírgula). Vazia ou ausente significa *todos*, que é o padrão de quem
hospeda a própria — a restrição existe para quem hospeda para os outros, não para quem clona.

**O filtro fica num ponto só: a entrada da réplica.** Um servidor fora da lista não é espelhado
no `guild_create`, e todo mutador da réplica já ignora servidor não espelhado. Consequência:
permissões, salas, tokens e o READY do gateway recusam sozinhos, sem uma checagem nova em cada
caminho — e o modo de falha é o que o ADR-0010 já exige, fechado.

**O bot fica no servidor e recusa com explicação.** `/tela` continua registrado mesmo onde não
se serve, e responde dizendo que aquele servidor não está autorizado e que o projeto é livre,
com o endereço do repositório. Um bot mudo parece quebrado; um bot que explica manda a pessoa
para o caminho certo, que é hospedar a própria instância.

**Um ID malformado para o processo no boot.** Entre servir todos e servir ninguém por causa de
uma vírgula errada, a única resposta que não escolhe um dos dois sozinha é não subir.

## Consequências

- Quem baixa o instalador público e não está num servidor listado vê uma recusa com motivo, não
  um aplicativo quebrado.
- Trocar a lista é editar o `.env.remote` e reiniciar; não exige build nem release nova.
- O interruptor "Public Bot" continua sendo a primeira camada, e continua fora do código. A
  lista é a camada que está no repositório e é revisável — e a que segura um convite antigo.
- Quem clona não precisa configurar nada: sem a variável, serve todos os servidores onde o bot
  estiver.
- Não há mensagem para quem *adicionou* o bot e nunca rodou `/tela`. Ele fica quieto até
  alguém tentar usar, e só aí explica.
