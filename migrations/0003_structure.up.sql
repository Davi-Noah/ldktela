-- SRS 5.2, bloco ESTRUTURA.

CREATE TABLE guilds (
    id                UUID PRIMARY KEY,
    name              VARCHAR(100) NOT NULL,
    icon_url          TEXT,
    owner_id          UUID NOT NULL REFERENCES users(id),
    discord_guild_id  BIGINT UNIQUE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE guild_members (
    guild_id   UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id)  ON DELETE CASCADE,
    nickname   VARCHAR(64),
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    banned_at  TIMESTAMPTZ,
    PRIMARY KEY (guild_id, user_id)
);
CREATE INDEX idx_members_user ON guild_members (user_id);

-- permissions: mascara de 63 bits. Constantes em SRS 5.3.
CREATE TABLE roles (
    id           UUID PRIMARY KEY,
    guild_id     UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name         VARCHAR(64) NOT NULL,
    color        VARCHAR(7),
    position     INT    NOT NULL DEFAULT 0,
    permissions  BIGINT NOT NULL DEFAULT 0,
    is_default   BOOLEAN NOT NULL DEFAULT FALSE,  -- cargo @everyone
    hoist        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_roles_default ON roles (guild_id) WHERE is_default;

CREATE TABLE member_roles (
    guild_id  UUID NOT NULL,
    user_id   UUID NOT NULL,
    role_id   UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (guild_id, user_id, role_id),
    FOREIGN KEY (guild_id, user_id) REFERENCES guild_members(guild_id, user_id) ON DELETE CASCADE
);

CREATE TABLE categories (
    id        UUID PRIMARY KEY,
    guild_id  UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name      VARCHAR(100) NOT NULL,
    position  INT NOT NULL DEFAULT 0
);

CREATE TYPE channel_type AS ENUM ('text', 'voice', 'dm', 'group_dm');

CREATE TABLE channels (
    id                  UUID PRIMARY KEY,
    guild_id            UUID REFERENCES guilds(id) ON DELETE CASCADE,  -- NULL em dm/group_dm
    category_id         UUID REFERENCES categories(id) ON DELETE SET NULL,
    name                VARCHAR(100) NOT NULL,
    topic               VARCHAR(1024),
    type                channel_type NOT NULL DEFAULT 'text',
    position            INT NOT NULL DEFAULT 0,
    discord_channel_id  BIGINT UNIQUE,
    bridge_enabled      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Canal de guild exige guild_id; conversa direta exige a ausencia dele.
    CONSTRAINT chk_channel_scope CHECK (
        (type IN ('text', 'voice')  AND guild_id IS NOT NULL) OR
        (type IN ('dm', 'group_dm') AND guild_id IS NULL AND category_id IS NULL)
    ),
    -- Ponte com Discord nunca se aplica a conversa direta (RF-18a).
    CONSTRAINT chk_bridge_scope CHECK (bridge_enabled = FALSE OR type = 'text')
);
CREATE INDEX idx_channels_guild ON channels (guild_id, position) WHERE guild_id IS NOT NULL;

-- Participantes de conversas diretas. Define acesso sozinho: sem cargos, sem overwrites.
-- Unicidade de DM 1:1 e garantida na aplicacao, resolvendo o canal existente
-- pelo par canonico de user_id antes de criar um novo.
CREATE TABLE channel_participants (
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    added_by    UUID REFERENCES users(id),
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    left_at     TIMESTAMPTZ,
    PRIMARY KEY (channel_id, user_id)
);
CREATE INDEX idx_participants_user ON channel_participants (user_id) WHERE left_at IS NULL;

CREATE TYPE overwrite_target AS ENUM ('role', 'member');

CREATE TABLE channel_overwrites (
    channel_id   UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    target_type  overwrite_target NOT NULL,
    target_id    UUID   NOT NULL,
    allow        BIGINT NOT NULL DEFAULT 0,
    deny         BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (channel_id, target_type, target_id)
);
