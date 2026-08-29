-- SRS 5.2, bloco VOZ.
-- UNLOGGED: estado efemero, nao precisa sobreviver a crash e nao gera WAL.
CREATE UNLOGGED TABLE voice_states (
    user_id     UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    session_id  TEXT NOT NULL,
    self_mute   BOOLEAN NOT NULL DEFAULT FALSE,
    self_deaf   BOOLEAN NOT NULL DEFAULT FALSE,
    streaming   BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_voice_channel ON voice_states (channel_id);
