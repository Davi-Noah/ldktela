//! `pairing_codes` (RF-01).
//!
//! A pairing code is a bearer credential. Only its hash is stored, and consuming
//! one is a single atomic statement: the guards live in the `WHERE`, so two
//! clients racing on the same code cannot both win.

use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::DbResult;

/// A consumed pairing code, with the identity it carried.
#[derive(Debug, Clone, Copy)]
pub struct ConsumedCode {
    pub discord_user_id: i64,
    pub discord_guild_id: i64,
}

pub async fn insert<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    code_hash: &str,
    discord_user_id: i64,
    discord_guild_id: i64,
    expires_at: OffsetDateTime,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO pairing_codes
            (id, code_hash, discord_user_id, discord_guild_id, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        id,
        code_hash,
        discord_user_id,
        discord_guild_id,
        expires_at,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Consume a code, atomically.
///
/// Returns `None` for every failure mode there is — unknown, expired, already
/// used — because the caller must not be able to tell them apart, and the only
/// way to guarantee that is to not know either.
pub async fn consume(
    pool: &PgPool,
    code_hash: &str,
    now: OffsetDateTime,
) -> DbResult<Option<ConsumedCode>> {
    let row = sqlx::query_as!(
        ConsumedCode,
        r#"
        UPDATE pairing_codes
           SET consumed_at = $2
         WHERE code_hash   = $1
           AND consumed_at IS NULL
           AND expires_at  > $2
        RETURNING discord_user_id, discord_guild_id
        "#,
        code_hash,
        now,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// How many codes this Discord account asked for since `since`.
///
/// Feeds the attempt limit: without it, a code is eight characters of guessable
/// surface that anyone can ask to have reissued indefinitely.
pub async fn recent_request_count(
    pool: &PgPool,
    discord_user_id: i64,
    since: OffsetDateTime,
) -> DbResult<i64> {
    let count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
          FROM pairing_codes
         WHERE discord_user_id = $1
           AND created_at      > $2
        "#,
        discord_user_id,
        since,
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Drop rows that can never be consumed again. Run from the maintenance job.
pub async fn delete_expired(pool: &PgPool, now: OffsetDateTime) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"DELETE FROM pairing_codes WHERE expires_at < $1 OR consumed_at IS NOT NULL"#,
        now,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
