//! `voice_states` (RF-20).
//!
//! The table is `UNLOGGED` (SRS §5.2): the state is ephemeral, does not need to
//! survive a crash and should not generate WAL. A restart losing every voice
//! state is correct — after a restart nobody is connected to the SFU either.

use protocol::voice::VoiceState;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::DbResult;

fn to_wire(
    user_id: Uuid,
    channel_id: Uuid,
    self_mute: bool,
    self_deaf: bool,
    streaming: bool,
) -> VoiceState {
    VoiceState {
        user_id,
        channel_id: Some(channel_id),
        self_mute,
        self_deaf,
        streaming,
    }
}

/// Places a user in a voice channel.
///
/// A user is in at most one channel at a time — `user_id` is the primary key —
/// so moving between rooms is an update, not a second row.
pub async fn join(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
    session_id: &str,
) -> DbResult<VoiceState> {
    let row = sqlx::query!(
        "INSERT INTO voice_states (user_id, channel_id, session_id) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (user_id) DO UPDATE \
           SET channel_id = EXCLUDED.channel_id, \
               session_id = EXCLUDED.session_id, \
               streaming = FALSE, \
               joined_at = NOW() \
         RETURNING user_id, channel_id, self_mute, self_deaf, streaming",
        user_id,
        channel_id,
        session_id
    )
    .fetch_one(pool)
    .await?;
    Ok(to_wire(
        row.user_id,
        row.channel_id,
        row.self_mute,
        row.self_deaf,
        row.streaming,
    ))
}

/// Removes a user from voice. `false` when they were not in a channel.
pub async fn leave(pool: &PgPool, user_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!("DELETE FROM voice_states WHERE user_id = $1", user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Empties a channel and reports who was in it, for the leave broadcast.
pub async fn clear_channel(pool: &PgPool, channel_id: Uuid) -> DbResult<Vec<Uuid>> {
    let rows = sqlx::query_scalar!(
        "DELETE FROM voice_states WHERE channel_id = $1 RETURNING user_id",
        channel_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Applies the two flags the client owns. `None` leaves a flag alone.
pub async fn set_flags(
    pool: &PgPool,
    user_id: Uuid,
    self_mute: Option<bool>,
    self_deaf: Option<bool>,
) -> DbResult<Option<VoiceState>> {
    let row = sqlx::query!(
        "UPDATE voice_states \
         SET self_mute = COALESCE($2, self_mute), \
             self_deaf = COALESCE($3, self_deaf) \
         WHERE user_id = $1 \
         RETURNING user_id, channel_id, self_mute, self_deaf, streaming",
        user_id,
        self_mute,
        self_deaf
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        to_wire(
            r.user_id,
            r.channel_id,
            r.self_mute,
            r.self_deaf,
            r.streaming,
        )
    }))
}

/// Screen share on or off, driven by the webhook rather than the client.
pub async fn set_streaming(
    pool: &PgPool,
    user_id: Uuid,
    streaming: bool,
) -> DbResult<Option<VoiceState>> {
    let row = sqlx::query!(
        "UPDATE voice_states SET streaming = $2 WHERE user_id = $1 \
         RETURNING user_id, channel_id, self_mute, self_deaf, streaming",
        user_id,
        streaming
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        to_wire(
            r.user_id,
            r.channel_id,
            r.self_mute,
            r.self_deaf,
            r.streaming,
        )
    }))
}

/// Voice states of every channel of a guild, for `READY`.
pub async fn list_by_guild(pool: &PgPool, guild_id: Uuid) -> DbResult<Vec<VoiceState>> {
    let rows = sqlx::query!(
        "SELECT v.user_id, v.channel_id, v.self_mute, v.self_deaf, v.streaming \
         FROM voice_states v \
         JOIN channels c ON c.id = v.channel_id \
         WHERE c.guild_id = $1",
        guild_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            to_wire(
                r.user_id,
                r.channel_id,
                r.self_mute,
                r.self_deaf,
                r.streaming,
            )
        })
        .collect())
}

/// How many people are in a channel right now.
pub async fn occupancy(pool: &PgPool, channel_id: Uuid) -> DbResult<i64> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM voice_states WHERE channel_id = $1"#,
        channel_id
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}
