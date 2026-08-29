-- SRS 5.2, bloco IDENTIDADE.
-- IDs sao UUIDv7 gerados na aplicacao: nenhuma PK tem DEFAULT (SRS 5.1, ADR-04).

CREATE TABLE users (
    id               UUID PRIMARY KEY,
    email            VARCHAR(255),
    username         VARCHAR(32)  NOT NULL,
    display_name     VARCHAR(64),
    password_hash    VARCHAR(255),
    avatar_url       TEXT,
    accent_color     VARCHAR(7),
    bio              VARCHAR(500),
    discord_user_id  BIGINT UNIQUE,
    is_migrated      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ
);
-- Ghost users nao tem email nem senha; contas reais exigem ambos.
CREATE UNIQUE INDEX idx_users_email     ON users (lower(email))    WHERE email IS NOT NULL;
CREATE UNIQUE INDEX idx_users_username  ON users (lower(username)) WHERE is_migrated = FALSE;
ALTER TABLE users ADD CONSTRAINT chk_real_user_credentials
    CHECK (is_migrated = TRUE OR (email IS NOT NULL AND password_hash IS NOT NULL));

CREATE TABLE invites (
    id           UUID PRIMARY KEY,
    code         VARCHAR(16) UNIQUE NOT NULL,
    created_by   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    guild_id     UUID,
    max_uses     INT NOT NULL DEFAULT 1,
    uses         INT NOT NULL DEFAULT 0,
    expires_at   TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Familia de refresh tokens: reuso de um token consumido revoga a familia inteira.
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
