//! `refresh_tokens` (RF-01a).
//!
//! Tokens belong to a **family**. Presenting a token that was already consumed
//! revokes the whole family, because the only way that happens is a stolen token
//! racing the legitimate client.

use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::DbResult;

#[derive(Debug, Clone)]
pub struct RefreshTokenRow {
    pub id: Uuid,
    pub family_id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub user_agent: Option<String>,
    pub issued_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub consumed_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
}

impl RefreshTokenRow {
    pub fn is_usable(&self, now: OffsetDateTime) -> bool {
        self.consumed_at.is_none() && self.revoked_at.is_none() && self.expires_at > now
    }
}

/// Issues a token. `family_id` equal to `id` starts a new family.
pub async fn insert<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    family_id: Uuid,
    user_id: Uuid,
    token_hash: &str,
    user_agent: Option<&str>,
    expires_at: OffsetDateTime,
) -> DbResult<RefreshTokenRow> {
    let row = sqlx::query_as!(
        RefreshTokenRow,
        r#"
        INSERT INTO refresh_tokens
            (id, family_id, user_id, token_hash, user_agent, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, family_id, user_id, token_hash, user_agent,
                  issued_at, expires_at, consumed_at, revoked_at
        "#,
        id,
        family_id,
        user_id,
        token_hash,
        user_agent,
        expires_at,
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

pub async fn find_by_hash<'e, E: PgExecutor<'e>>(
    executor: E,
    token_hash: &str,
) -> DbResult<Option<RefreshTokenRow>> {
    let row = sqlx::query_as!(
        RefreshTokenRow,
        r#"
        SELECT id, family_id, user_id, token_hash, user_agent,
               issued_at, expires_at, consumed_at, revoked_at
        FROM refresh_tokens WHERE token_hash = $1
        "#,
        token_hash
    )
    .fetch_optional(executor)
    .await?;
    Ok(row)
}

/// Marks a token consumed, but only if it was not consumed yet.
///
/// The `consumed_at IS NULL` guard is what makes rotation safe under
/// concurrency: two simultaneous refreshes with the same token produce exactly
/// one winner, and the loser sees `Ok(false)` — which the caller treats as reuse.
pub async fn consume<'e, E: PgExecutor<'e>>(executor: E, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        "UPDATE refresh_tokens SET consumed_at = NOW() \
         WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL",
        id
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Revokes every token in a family. Returns how many were still live.
pub async fn revoke_family<'e, E: PgExecutor<'e>>(executor: E, family_id: Uuid) -> DbResult<u64> {
    let result = sqlx::query!(
        "UPDATE refresh_tokens SET revoked_at = NOW() \
         WHERE family_id = $1 AND revoked_at IS NULL",
        family_id
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected())
}

/// Revokes every live token of a user, across families. Used on logout-all and
/// on password change.
pub async fn revoke_all_for_user(pool: &PgPool, user_id: Uuid) -> DbResult<u64> {
    let result = sqlx::query!(
        "UPDATE refresh_tokens SET revoked_at = NOW() \
         WHERE user_id = $1 AND revoked_at IS NULL",
        user_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Every token of a family, oldest first. Used by tests and by audit.
pub async fn list_family(pool: &PgPool, family_id: Uuid) -> DbResult<Vec<RefreshTokenRow>> {
    let rows = sqlx::query_as!(
        RefreshTokenRow,
        r#"
        SELECT id, family_id, user_id, token_hash, user_agent,
               issued_at, expires_at, consumed_at, revoked_at
        FROM refresh_tokens WHERE family_id = $1 ORDER BY id
        "#,
        family_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Housekeeping: drops tokens that expired more than `grace_days` ago.
pub async fn delete_expired(pool: &PgPool, grace_days: i32) -> DbResult<u64> {
    let result = sqlx::query!(
        "DELETE FROM refresh_tokens WHERE expires_at < NOW() - make_interval(days => $1)",
        grace_days
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
