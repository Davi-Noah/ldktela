# ADR-0016 — Poda do escopo por reescrita das migrations

- **Status:** **Proposto** — exige aval humano explícito (`CLAUDE.md` §10)
- **Data:** 2026-09-12

## Contexto

O pivô do [ADR-0008](0008-complemento-ao-discord.md) torna obsoleta boa parte do schema:

| Migration | Destino |
|---|---|
| `0001_extensions` | `pg_trgm` existia para busca parcial. Sai. |
| `0002_identity` | Sobrevive reduzida: sem `invites`, sem `email`/`password_hash` ([ADR-0009](0009-identidade-por-pareamento.md)). |
| `0003_structure` | Quase toda fora: cargos, overwrites, categorias, membros, participantes ([ADR-0010](0010-autorizacao-derivada-do-discord.md)). |
| `0004_messages` | Fora inteira: mensagens, anexos, reações, menções, estado de leitura. |
| `0005_voice` | Sobrevive, adaptada ao snowflake do Discord ([ADR-0011](0011-sala-e-o-canal-de-voz.md)). |
| `0006_bridge` | Fora inteira. As três tabelas nunca tiveram uma linha de código Rust que as lesse. |

Há dois caminhos, e a diferença entre eles não é técnica, é sobre o que a história de
migrations deve contar.

## Decisão proposta

Reescrever o conjunto de migrations do zero para o schema novo, em vez de acumular
migrations de `DROP`.

O fundamento é que **não existe implantação em produção e não existe dado a preservar**:
o produto nunca rodou fora da máquina de desenvolvimento. Uma cadeia de doze arquivos que
cria vinte tabelas e depois derruba quinze documenta um produto que deixou de existir, e
todo `just migrate` do zero passaria a pagar esse histórico.

## Consequências

- A cadeia de migrations passa a descrever o produto atual, legível de cima a baixo.
- Qualquer banco de desenvolvimento existente precisa ser recriado. Como o único conteúdo
  é dado de teste, o custo é um `docker compose down -v`.
- `.sqlx/` é regenerado inteiro, e os tipos TypeScript também.
- **Ponto de não retorno:** se em algum momento existir uma instância com dados reais,
  esta decisão deixa de ser aplicável e a poda tem de virar migration aditiva.

## Alternativas rejeitadas

- **Migrations aditivas de `DROP` (0007 em diante).** É o caminho correto quando há dados,
  e o errado aqui: preserva um histórico que só descreve trabalho descartado, e deixa o
  schema real espalhado por doze arquivos em vez de três.

## Por que está como Proposto

`CLAUDE.md` §10 exige confirmação humana para qualquer migration destrutiva. Este ADR
descreve a recomendação; ele só passa a `Aceito` com o aval explícito do dono do projeto,
e a poda não é executada antes disso.
