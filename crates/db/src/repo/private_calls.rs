use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::DbResult;

#[derive(Debug, Clone, Copy)]
pub struct PrivateCallRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub guest_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
}

pub async fn insert(
    pool: &PgPool,
    id: Uuid,
    owner_id: Uuid,
    invite_hash: &str,
    invite_expires_at: OffsetDateTime,
) -> DbResult<PrivateCallRow> {
    let row = sqlx::query_as!(
        PrivateCallRow,
        r#"INSERT INTO private_calls (id, owner_id, invite_hash, invite_expires_at)
           VALUES ($1, $2, $3, $4)
           RETURNING id, owner_id, guest_id, created_at, ended_at"#,
        id,
        owner_id,
        invite_hash,
        invite_expires_at
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn join(
    pool: &PgPool,
    invite_hash: &str,
    guest_id: Uuid,
    now: OffsetDateTime,
) -> DbResult<Option<PrivateCallRow>> {
    let row = sqlx::query_as!(
        PrivateCallRow,
        r#"UPDATE private_calls
              SET guest_id = $2, invite_consumed_at = $3
            WHERE invite_hash = $1
              AND owner_id <> $2
              AND guest_id IS NULL
              AND invite_consumed_at IS NULL
              AND invite_expires_at > $3
              AND ended_at IS NULL
        RETURNING id, owner_id, guest_id, created_at, ended_at"#,
        invite_hash,
        guest_id,
        now
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn find_for_user(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<PrivateCallRow>> {
    let row = sqlx::query_as!(
        PrivateCallRow,
        r#"SELECT id, owner_id, guest_id, created_at, ended_at
             FROM private_calls
            WHERE id = $1 AND (owner_id = $2 OR guest_id = $2)"#,
        id,
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn find_active_for_user(
    pool: &PgPool,
    user_id: Uuid,
) -> DbResult<Option<PrivateCallRow>> {
    let row = sqlx::query_as!(
        PrivateCallRow,
        r#"SELECT id, owner_id, guest_id, created_at, ended_at
             FROM private_calls
            WHERE (owner_id = $1 OR guest_id = $1) AND ended_at IS NULL
            ORDER BY created_at DESC
            LIMIT 1"#,
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn end(
    pool: &PgPool,
    id: Uuid,
    owner_id: Uuid,
    now: OffsetDateTime,
) -> DbResult<Option<PrivateCallRow>> {
    let row = sqlx::query_as!(
        PrivateCallRow,
        r#"UPDATE private_calls SET ended_at = $3
            WHERE id = $1 AND owner_id = $2 AND ended_at IS NULL
        RETURNING id, owner_id, guest_id, created_at, ended_at"#,
        id,
        owner_id,
        now
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
