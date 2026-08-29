-- SRS 5.2, bloco MENSAGENS.

CREATE TABLE messages (
    id           UUID PRIMARY KEY,                 -- UUIDv7: ordena por tempo
    channel_id   UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    author_id    UUID NOT NULL REFERENCES users(id),
    content      TEXT NOT NULL DEFAULT '',         -- vazio e valido: mensagem so-anexo
    reply_to_id  UUID REFERENCES messages(id) ON DELETE SET NULL,
    is_pinned    BOOLEAN NOT NULL DEFAULT FALSE,
    edited_at    TIMESTAMPTZ,
    deleted_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_messages_channel     ON messages (channel_id, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_messages_author      ON messages (author_id);
CREATE INDEX idx_messages_pinned      ON messages (channel_id) WHERE is_pinned;
-- Busca full-text (RF-17). Custo: 30-50% do tamanho da tabela, ja previsto no RNF-08.
CREATE INDEX idx_messages_fts ON messages
    USING GIN (to_tsvector('portuguese', content)) WHERE deleted_at IS NULL;
-- Trigrama (busca parcial/typo) desativado por padrao: dobraria o custo de indice.
-- Habilitar so se a busca exata se mostrar insuficiente em uso real (P-02).
-- CREATE INDEX idx_messages_trgm ON messages
--     USING GIN (content gin_trgm_ops) WHERE deleted_at IS NULL;

CREATE TABLE attachments (
    id            UUID PRIMARY KEY,
    message_id    UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    r2_key        TEXT,             -- NULL = anexo nao migrado (placeholder, RF-25a)
    skip_reason   VARCHAR(32),      -- ex.: 'video_fora_do_orcamento', 'cdn_expirada'
    filename      VARCHAR(255) NOT NULL,
    content_type  VARCHAR(100) NOT NULL,
    size_bytes    BIGINT NOT NULL,
    width         INT,     -- persistido p/ reservar espaco e evitar reflow (RNF-04)
    height        INT,
    source_url    TEXT,    -- URL original do Discord, apenas p/ auditoria da migracao
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_attachments_message ON attachments (message_id);

CREATE TABLE reactions (
    message_id  UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    emoji       VARCHAR(32) NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (message_id, user_id, emoji)
);

CREATE TABLE mentions (
    message_id    UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    user_id       UUID REFERENCES users(id) ON DELETE CASCADE,
    role_id       UUID REFERENCES roles(id) ON DELETE CASCADE,
    is_everyone   BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX idx_mentions_user ON mentions (user_id);

CREATE TABLE read_states (
    user_id               UUID NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    channel_id            UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    last_read_message_id  UUID,
    mention_count         INT NOT NULL DEFAULT 0,
    muted                 BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, channel_id)
);
