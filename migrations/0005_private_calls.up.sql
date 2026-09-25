CREATE TABLE oauth_login_attempts (
    id           UUID PRIMARY KEY,
    state_hash   CHAR(64) UNIQUE NOT NULL,
    poll_hash    CHAR(64) NOT NULL,
    user_id      UUID REFERENCES users(id) ON DELETE CASCADE,
    expires_at   TIMESTAMPTZ NOT NULL,
    consumed_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_oauth_login_pending ON oauth_login_attempts (expires_at)
    WHERE consumed_at IS NULL;

CREATE TABLE private_calls (
    id                   UUID PRIMARY KEY,
    owner_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    guest_id             UUID REFERENCES users(id) ON DELETE CASCADE,
    invite_hash          CHAR(64) UNIQUE NOT NULL,
    invite_expires_at    TIMESTAMPTZ NOT NULL,
    invite_consumed_at   TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at             TIMESTAMPTZ,
    CONSTRAINT private_call_distinct_members CHECK (guest_id IS NULL OR guest_id <> owner_id)
);
CREATE INDEX idx_private_calls_owner_active ON private_calls (owner_id) WHERE ended_at IS NULL;
CREATE INDEX idx_private_calls_guest_active ON private_calls (guest_id) WHERE ended_at IS NULL;

