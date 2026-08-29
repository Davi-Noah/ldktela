//! Reactions, mentions and read state — everything attached to a message that
//! is not the message (RF-15, RF-16).

use domain::Mentions;
use protocol::channel::ReadState;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::error::DbResult;

// ---------------------------------------------------------------------------
// Reações (RF-15)
// ---------------------------------------------------------------------------

/// Adds a reaction. Idempotent: the primary key is
/// `(message_id, user_id, emoji)`, so reacting twice is one reaction.
/// Returns `true` when the row was new, so the caller only dispatches once.
pub async fn add_reaction(
    pool: &PgPool,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> DbResult<bool> {
    let result = sqlx::query!(
        "INSERT INTO reactions (message_id, user_id, emoji) VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING",
        message_id,
        user_id,
        emoji
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn remove_reaction(
    pool: &PgPool,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> DbResult<bool> {
    let result = sqlx::query!(
        "DELETE FROM reactions WHERE message_id = $1 AND user_id = $2 AND emoji = $3",
        message_id,
        user_id,
        emoji
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

// ---------------------------------------------------------------------------
// Menções
// ---------------------------------------------------------------------------

/// Replaces the mention rows of a message.
///
/// A replace, not an append, because editing a message can remove a mention —
/// and `mentions` has no primary key in SRS §5.2, so appending would duplicate.
/// Takes a connection rather than an executor because it runs several
/// statements that must land together.
pub async fn replace_mentions(
    conn: &mut sqlx::PgConnection,
    message_id: Uuid,
    mentions: &Mentions,
) -> DbResult<()> {
    sqlx::query!("DELETE FROM mentions WHERE message_id = $1", message_id)
        .execute(&mut *conn)
        .await?;

    for user_id in &mentions.users {
        sqlx::query!(
            "INSERT INTO mentions (message_id, user_id) VALUES ($1, $2)",
            message_id,
            user_id
        )
        .execute(&mut *conn)
        .await?;
    }
    for role_id in &mentions.roles {
        sqlx::query!(
            "INSERT INTO mentions (message_id, role_id) VALUES ($1, $2)",
            message_id,
            role_id
        )
        .execute(&mut *conn)
        .await?;
    }
    if mentions.everyone {
        sqlx::query!(
            "INSERT INTO mentions (message_id, is_everyone) VALUES ($1, TRUE)",
            message_id
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Members reached by a message's mentions, expanding roles to their holders and
/// `@everyone` to the channel's viewers.
///
/// `viewers` is passed in rather than resolved here: the caller already computed
/// it to decide the fan-out, and mentioning someone who cannot see the channel
/// must not notify them.
pub async fn mentioned_members(
    pool: &PgPool,
    mentions: &Mentions,
    viewers: &[Uuid],
    guild_id: Option<Uuid>,
) -> DbResult<Vec<Uuid>> {
    let mut reached: Vec<Uuid> = Vec::new();
    if mentions.everyone {
        reached.extend_from_slice(viewers);
    }
    reached.extend(mentions.users.iter().copied());

    if !mentions.roles.is_empty() {
        if let Some(guild_id) = guild_id {
            let role_ids: Vec<Uuid> = mentions.roles.iter().copied().collect();
            let holders = sqlx::query_scalar!(
                "SELECT user_id FROM member_roles WHERE guild_id = $1 AND role_id = ANY($2)",
                guild_id,
                &role_ids
            )
            .fetch_all(pool)
            .await?;
            reached.extend(holders);
        }
    }

    reached.retain(|id| viewers.contains(id));
    reached.sort_unstable();
    reached.dedup();
    Ok(reached)
}

// ---------------------------------------------------------------------------
// Estado de leitura (RF-16)
// ---------------------------------------------------------------------------

/// Bumps a user's mention counter for a channel.
///
/// The counter is the server's (`docs/protocol/websocket.md` §6.5): the client
/// displays it and never recomputes it, so two machines cannot disagree.
pub async fn increment_mentions(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
) -> DbResult<ReadState> {
    let row = sqlx::query!(
        "INSERT INTO read_states (user_id, channel_id, mention_count) \
         VALUES ($1, $2, 1) \
         ON CONFLICT (user_id, channel_id) \
         DO UPDATE SET mention_count = read_states.mention_count + 1, updated_at = NOW() \
         RETURNING channel_id, last_read_message_id, mention_count, muted",
        user_id,
        channel_id
    )
    .fetch_one(pool)
    .await?;
    Ok(ReadState {
        channel_id: row.channel_id,
        last_read_message_id: row.last_read_message_id,
        mention_count: row.mention_count,
        muted: row.muted,
    })
}

/// Marks a channel read up to a message and recounts the mentions left after it.
///
/// Recounting rather than zeroing: a mention that arrived between the client's
/// last render and this call must survive, or it disappears unread.
pub async fn mark_read(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    last_read_message_id: Uuid,
) -> DbResult<ReadState> {
    let mut tx = pool.begin().await?;
    let remaining = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
        FROM mentions m
        JOIN messages msg ON msg.id = m.message_id
        WHERE msg.channel_id = $1
          AND msg.id > $2
          AND msg.deleted_at IS NULL
          AND m.user_id = $3
        "#,
        channel_id,
        last_read_message_id,
        user_id
    )
    .fetch_one(&mut *tx)
    .await?;

    let row = sqlx::query!(
        "INSERT INTO read_states (user_id, channel_id, last_read_message_id, mention_count) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, channel_id) \
         DO UPDATE SET last_read_message_id = EXCLUDED.last_read_message_id, \
                       mention_count = EXCLUDED.mention_count, \
                       updated_at = NOW() \
         RETURNING channel_id, last_read_message_id, mention_count, muted",
        user_id,
        channel_id,
        last_read_message_id,
        remaining as i32
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(ReadState {
        channel_id: row.channel_id,
        last_read_message_id: row.last_read_message_id,
        mention_count: row.mention_count,
        muted: row.muted,
    })
}

pub async fn read_state(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
) -> DbResult<Option<ReadState>> {
    let row = sqlx::query!(
        "SELECT channel_id, last_read_message_id, mention_count, muted \
         FROM read_states WHERE user_id = $1 AND channel_id = $2",
        user_id,
        channel_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| ReadState {
        channel_id: r.channel_id,
        last_read_message_id: r.last_read_message_id,
        mention_count: r.mention_count,
        muted: r.muted,
    }))
}

// ---------------------------------------------------------------------------
// Anexos
// ---------------------------------------------------------------------------

/// One attachment being persisted alongside a message.
#[derive(Debug, Clone)]
pub struct NewAttachment<'a> {
    pub id: Uuid,
    pub message_id: Uuid,
    pub r2_key: Option<&'a str>,
    pub skip_reason: Option<&'a str>,
    pub filename: &'a str,
    pub content_type: &'a str,
    pub size_bytes: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub source_url: Option<&'a str>,
}

pub async fn insert_attachment<'e, E: PgExecutor<'e>>(
    executor: E,
    attachment: NewAttachment<'_>,
) -> DbResult<Uuid> {
    let id = sqlx::query_scalar!(
        "INSERT INTO attachments \
            (id, message_id, r2_key, skip_reason, filename, content_type, \
             size_bytes, width, height, source_url) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
        attachment.id,
        attachment.message_id,
        attachment.r2_key,
        attachment.skip_reason,
        attachment.filename,
        attachment.content_type,
        attachment.size_bytes,
        attachment.width,
        attachment.height,
        attachment.source_url,
    )
    .fetch_one(executor)
    .await?;
    Ok(id)
}
