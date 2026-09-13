# ADR-0002 — SFU LiveKit auto-hospedado na mesma VM

- **Status:** Aceito
- **Data:** 2026-08-29 (originado no SRS v1.2 §2.1, C-01)

## Contexto

A premissa original de usar LiveKit Cloud partia de uma cota gratuita de 50.000 minutos
de WebRTC por mês. O número real é **5.000**, e é teto rígido: ao estourar, novas conexões
passam a falhar. Com compartilhamento de tela como uso central, 5.000 minutos são poucas
horas por mês.

## Decisão

O SFU LiveKit roda auto-hospedado, na mesma VM do backend.

## Consequências

- O limite deixa de ser minutos e passa a ser **egress mensal**, que é o recurso realmente
  abundante na VM (10 TB/mês). O orçamento vira um RNF mensurável em vez de um teto opaco.
- A A1.Flex aloca ~1 Gbps por OCPU, então banda instantânea não é gargalo para o perfil de
  uso (um publicador, poucos espectadores).
- Passamos a ser responsáveis pelo ciclo de vida do SFU: portas, TURN, certificado,
  atualização. O [ADR-0013](0013-turn-tls-443-primario.md) trata a parte mais crítica disso.
- Com o pivô para screen-share-only ([ADR-0008](0008-complemento-ao-discord.md)) esta
  decisão fica **mais** justificada, não menos: o egress passa a ser o único custo variável
  do produto inteiro.

## Alternativas rejeitadas

- **LiveKit Cloud.** Cota real inviável para o perfil de uso, e falha fechada ao estourar.
