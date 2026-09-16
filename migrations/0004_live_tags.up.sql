-- RF-38 a RF-40: a tag `[LIVE]` no apelido do Discord (ADR-0024).
--
-- O apelido e estado do usuario no servidor DELE, e o ADR exige restaura-lo
-- exatamente -- inclusive o caso de nao haver apelido nenhum, que precisa voltar
-- a nao haver, e nao virar o nome de usuario gravado como apelido.
--
-- Por isso esta tabela existe, e por isso ela NAO e UNLOGGED como `room_presence`:
-- ela so tem valor se sobreviver justamente a queda que o RF-40 trata. Guardar
-- isto em memoria seria guardar no unico lugar que some quando se precisa dele.
CREATE TABLE live_tags (
    discord_guild_id BIGINT      NOT NULL,
    discord_user_id  BIGINT      NOT NULL,
    -- NULL significa "nao tinha apelido", e e diferente de string vazia.
    previous_nick    TEXT,
    tagged_at        TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (discord_guild_id, discord_user_id)
);
