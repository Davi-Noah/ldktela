-- Share session history. Feeds the egress budget (RNF-05) and the publisher's
-- own history.
--
-- peak_viewers is a COUNT, deliberately. Who watched what is not recorded
-- (RNF-08): the product needs to size egress, not to build an audience log.
CREATE TABLE share_sessions (
    id                  UUID PRIMARY KEY,
    discord_channel_id  BIGINT NOT NULL,
    publisher_id        UUID   NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    started_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at            TIMESTAMPTZ,
    peak_viewers        INT    NOT NULL DEFAULT 0,
    egress_bytes        BIGINT NOT NULL DEFAULT 0
);

-- One open session per publisher per channel: a second track_published for the
-- same publisher must update the open row, not open a parallel one.
CREATE UNIQUE INDEX idx_sessions_open_publisher
    ON share_sessions (discord_channel_id, publisher_id) WHERE ended_at IS NULL;
CREATE INDEX idx_sessions_month ON share_sessions (started_at);
