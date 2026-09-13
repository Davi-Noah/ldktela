-- Who is in which room right now.
--
-- UNLOGGED on purpose: without a socket there is no presence, so this must not
-- survive a crash, and it should not generate WAL for state that is rebuilt on
-- reconnect.
-- Presence means "connected to our SFU room", not "in the Discord voice
-- channel". The two differ for anyone who is in the call but has not opened the
-- app, and the useful question is who can actually see the screen.
CREATE UNLOGGED TABLE room_presence (
    user_id             UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    discord_channel_id  BIGINT  NOT NULL,
    publishing          BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_presence_channel ON room_presence (discord_channel_id);
