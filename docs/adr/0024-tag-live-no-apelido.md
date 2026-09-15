# ADR-0024 — A tag `[LIVE]` vai no apelido do Discord, com guardas

- **Status:** Aceito
- **Data:** 2026-09-14

## Contexto

Quando alguém usa o Go Live do Discord, o Discord marca a pessoa na lista de voz.
Como é justamente esse recurso que está desligado na região alvo, quem compartilha
pelo nosso produto fica indistinguível de quem só está na chamada.

O S8 pede prefixar `[LIVE] ` no apelido de quem está transmitindo.

Duas limitações são do Discord e não têm contorno:

1. **O bot não pode alterar o apelido do dono do servidor.** Nunca, por nenhuma
   permissão. No servidor de teste, o dono é o próprio dono do projeto.
2. **O bot não pode alterar o apelido de quem tem cargo acima do dele.** Isso
   tem contorno: mover o cargo do bot para o topo da hierarquia de cargos.

Há ainda um risco que é nosso: renomear muta estado do usuário no servidor dele.
Se o processo cair com alguém marcado, o apelido fica sujo.

## Decisão

A tag é aplicada, com quatro guardas obrigatórias:

1. **Hierarquia verificada antes de tentar.** Dono do servidor e cargos acima do
   bot são pulados em silêncio para o usuário e com log claro para o operador —
   não se tenta e falha, não se avisa a sala.
2. **Apelido anterior salvo e restaurado exatamente.** Inclusive o caso de não
   haver apelido, que deve voltar a não haver — e não virar o nome de usuário
   gravado como apelido.
3. **Limpeza no arranque.** O servidor varre os membros marcados e desmarca o que
   sobrou de uma queda, antes de aceitar qualquer sessão nova.
4. **Idempotência.** Marcar quem já está marcado não empilha prefixo; desmarcar
   quem não está não faz nada.

O produto exige `MANAGE_NICKNAMES` e recomenda, na documentação de instalação,
posicionar o cargo do bot acima dos cargos de membro.

## Consequências

- Quem for dono do servidor nunca recebe a tag. É limitação do Discord e precisa
  estar escrita no `DESTRAVAR.md`, ou vira relato de bug.
- O anúncio no canal ([ADR-0008](0008-complemento-ao-discord.md), fatia S8)
  continua sendo o sinal principal, e é o que funciona para todo mundo. A tag é
  reforço, não substituto — e essa ordem importa se algum dia a tag for removida.
- O apelido é estado do usuário no servidor **dele**. Errar a restauração é
  estragar algo que não é nosso, e por isso a restauração vem antes da marcação
  na ordem de implementação e de teste.
- O limite de taxa de edição de membro passa a valer no caminho de começar e
  parar de compartilhar. Marcar e desmarcar em rajada precisa de coalescência.

## Alternativas rejeitadas

- **Só o anúncio no canal.** Mais seguro e menos visível: quem está olhando a
  lista de voz não vê nada. O pedido é justamente essa visibilidade.
- **Renomear o canal de voz.** O limite é de duas renomeações por 10 minutos por
  canal, o que quebra sob uso normal. Já rejeitado na fatia S8.
