-- Identity. There is no password and no email: the Discord account is the
-- identity, reached through a pairing code issued by the bot (ADR-0009).

-- Cache of the Discord profile. Refreshed on pairing and on gateway events.
CREATE TABLE users (
    id               UUID PRIMARY KEY,
    discord_user_id  BIGINT UNIQUE NOT NULL,
    username         VARCHAR(32)  NOT NULL,
    display_name     VARCHAR(64),
    avatar_url       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ
);

-- Short-lived, single-use credential. Only the hash is stored: a leaked dump
-- must not hand out live pairing codes.
CREATE TABLE pairing_codes (
    id                UUID PRIMARY KEY,
    code_hash         CHAR(64) UNIQUE NOT NULL,
    discord_user_id   BIGINT      NOT NULL,
    discord_guild_id  BIGINT      NOT NULL,
    expires_at        TIMESTAMPTZ NOT NULL,
    consumed_at       TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_pairing_pending ON pairing_codes (expires_at) WHERE consumed_at IS NULL;
CREATE INDEX idx_pairing_user    ON pairing_codes (discord_user_id, created_at DESC);

-- Refresh token families: replaying a consumed token revokes the whole family.
-- Carried over unchanged from v1; only what happens before it changed.
CREATE TABLE refresh_tokens (
    id          UUID PRIMARY KEY,
    family_id   UUID NOT NULL,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  CHAR(64) UNIQUE NOT NULL,
    user_agent  TEXT,
    issued_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at  TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ
);
CREATE INDEX idx_refresh_family ON refresh_tokens (family_id);
CREATE INDEX idx_refresh_user   ON refresh_tokens (user_id) WHERE revoked_at IS NULL;
