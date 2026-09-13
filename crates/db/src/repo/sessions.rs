//! `share_sessions` (RNF-05, RNF-08).
//!
//! One row per screen share, from first track to last. This is the only place
//! the product keeps history, and it keeps the minimum: who published, when, for
//! how long, how many watched at peak, and how many bytes went out.
//!
//! Deliberately absent: who watched. A per-viewer log is not needed to size
//! egress, and building one would make this a surveillance record of a product
//! whose whole point is being usable where that is dangerous.

use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::DbResult;

#[derive(Debug, Clone, Copy)]
pub struct ShareSessionRow {
    pub id: Uuid,
    pub discord_channel_id: i64,
    pub publisher_id: Uuid,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    pub peak_viewers: i32,
    pub egress_bytes: i64,
}

/// Open a session, or return the one already open.
///
/// Sharing screen *and* screen audio publishes two LiveKit tracks, so this is
/// called twice for one session. The partial unique index makes the second call
/// a no-op instead of a second row.
pub async fn open<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    discord_channel_id: i64,
    publisher_id: Uuid,
) -> DbResult<ShareSessionRow> {
    let row = sqlx::query_as!(
        ShareSessionRow,
        r#"
        INSERT INTO share_sessions (id, discord_channel_id, publisher_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (discord_channel_id, publisher_id) WHERE ended_at IS NULL
        DO UPDATE SET discord_channel_id = share_sessions.discord_channel_id
        RETURNING id, discord_channel_id, publisher_id, started_at, ended_at,
                  peak_viewers, egress_bytes
        "#,
        id,
        discord_channel_id,
        publisher_id,
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

/// Close the open session for this publisher. Idempotent: a second unpublish
/// finds nothing to close and says so with `None`.
pub async fn close<'e, E: PgExecutor<'e>>(
    executor: E,
    discord_channel_id: i64,
    publisher_id: Uuid,
    now: OffsetDateTime,
) -> DbResult<Option<Uuid>> {
    let id = sqlx::query_scalar!(
        r#"
        UPDATE share_sessions
           SET ended_at = $3
         WHERE discord_channel_id = $1
           AND publisher_id       = $2
           AND ended_at IS NULL
        RETURNING id
        "#,
        discord_channel_id,
        publisher_id,
        now,
    )
    .fetch_optional(executor)
    .await?;
    Ok(id)
}

/// Raise the peak viewer count if the current audience is larger.
pub async fn observe_viewers<'e, E: PgExecutor<'e>>(
    executor: E,
    discord_channel_id: i64,
    viewers: i32,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        UPDATE share_sessions
           SET peak_viewers = GREATEST(peak_viewers, $2)
         WHERE discord_channel_id = $1
           AND ended_at IS NULL
        "#,
        discord_channel_id,
        viewers,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Add measured egress to a session.
pub async fn add_egress<'e, E: PgExecutor<'e>>(
    executor: E,
    session_id: Uuid,
    bytes: i64,
) -> DbResult<()> {
    sqlx::query!(
        r#"UPDATE share_sessions SET egress_bytes = egress_bytes + $2 WHERE id = $1"#,
        session_id,
        bytes,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Total egress since an instant. This is the number RNF-05 caps, and the reason
/// the table exists at all.
pub async fn egress_since(pool: &PgPool, since: OffsetDateTime) -> DbResult<i64> {
    let total = sqlx::query_scalar!(
        r#"
        -- SUM sobre BIGINT devolve NUMERIC no Postgres; o cast evita arrastar a
        -- feature bigdecimal do SQLx para dentro do build por causa de uma query.
        SELECT COALESCE(SUM(egress_bytes), 0)::BIGINT AS "total!"
          FROM share_sessions
         WHERE started_at >= $1
        "#,
        since,
    )
    .fetch_one(pool)
    .await?;
    Ok(total)
}

/// Close every session left open by a crash. Called once at startup: a row with
/// no end time and no live SFU room is a lie the next egress report would repeat.
pub async fn close_all_open(pool: &PgPool, now: OffsetDateTime) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"UPDATE share_sessions SET ended_at = $1 WHERE ended_at IS NULL"#,
        now,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
