-- SRS 5.2, bloco PONTE DISCORD.

CREATE TYPE message_origin AS ENUM ('internal', 'discord');

CREATE TABLE message_mappings (
    internal_message_id  UUID PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    discord_message_id   BIGINT UNIQUE NOT NULL,
    channel_id           UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    origin               message_origin NOT NULL,
    synced_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_mappings_discord ON message_mappings (discord_message_id);

-- Token do webhook NAO e armazenado aqui em texto puro: ver RNF-15.
CREATE TABLE channel_webhooks (
    channel_id          UUID PRIMARY KEY REFERENCES channels(id) ON DELETE CASCADE,
    discord_webhook_id  BIGINT NOT NULL,
    token_ref           TEXT NOT NULL,   -- referencia a variavel de ambiente/cofre
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fila persistente de saida da ponte (RF-31). Sobrevive a reinicio do processo.
CREATE TABLE bridge_outbox (
    id             UUID PRIMARY KEY,
    channel_id     UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    message_id     UUID REFERENCES messages(id) ON DELETE CASCADE,
    action         VARCHAR(16) NOT NULL,   -- create | update | delete
    payload        JSONB NOT NULL,
    attempts       INT NOT NULL DEFAULT 0,
    next_retry_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at   TIMESTAMPTZ,
    last_error     TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_outbox_pending ON bridge_outbox (next_retry_at) WHERE delivered_at IS NULL;
