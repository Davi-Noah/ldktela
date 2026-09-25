use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::DbResult;

pub async fn insert(
    pool: &PgPool,
    id: Uuid,
    state_hash: &str,
    poll_hash: &str,
    expires_at: OffsetDateTime,
) -> DbResult<()> {
    sqlx::query!(
        r#"INSERT INTO oauth_login_attempts (id, state_hash, poll_hash, expires_at)
           VALUES ($1, $2, $3, $4)"#,
        id,
        state_hash,
        poll_hash,
        expires_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn is_pending(pool: &PgPool, state_hash: &str, now: OffsetDateTime) -> DbResult<bool> {
    let pending = sqlx::query_scalar!(
        r#"SELECT EXISTS(
               SELECT 1 FROM oauth_login_attempts
                WHERE state_hash = $1
                  AND user_id IS NULL
                  AND consumed_at IS NULL
                  AND expires_at > $2
           ) AS "pending!""#,
        state_hash,
        now
    )
    .fetch_one(pool)
    .await?;
    Ok(pending)
}

pub async fn complete(
    pool: &PgPool,
    state_hash: &str,
    user_id: Uuid,
    now: OffsetDateTime,
) -> DbResult<bool> {
    let result = sqlx::query!(
        r#"UPDATE oauth_login_attempts
              SET user_id = $2
            WHERE state_hash = $1
              AND user_id IS NULL
              AND consumed_at IS NULL
              AND expires_at > $3"#,
        state_hash,
        user_id,
        now
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn consume(
    pool: &PgPool,
    id: Uuid,
    poll_hash: &str,
    now: OffsetDateTime,
) -> DbResult<Option<Uuid>> {
    let user_id = sqlx::query_scalar!(
        r#"UPDATE oauth_login_attempts
              SET consumed_at = $3
            WHERE id = $1
              AND poll_hash = $2
              AND user_id IS NOT NULL
              AND consumed_at IS NULL
              AND expires_at > $3
        RETURNING user_id AS "user_id!""#,
        id,
        poll_hash,
        now
    )
    .fetch_optional(pool)
    .await?;
    Ok(user_id)
}
