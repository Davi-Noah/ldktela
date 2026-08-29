//! `invites` (RF-02).

use protocol::guild::Invite;
use protocol::scalars::Timestamp;
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{missing, DbResult};

#[derive(Debug, Clone)]
pub struct InviteRow {
    pub id: Uuid,
    pub code: String,
    pub created_by: Uuid,
    pub guild_id: Option<Uuid>,
    pub max_uses: i32,
    pub uses: i32,
    pub expires_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

impl InviteRow {
    /// Usable right now: not revoked, not expired, uses left.
    pub fn is_valid(&self, now: OffsetDateTime) -> bool {
        self.revoked_at.is_none()
            && self.expires_at.is_none_or(|at| at > now)
            && self.uses < self.max_uses
    }

    pub fn to_wire(&self) -> Invite {
        Invite {
            id: self.id,
            code: self.code.clone(),
            created_by: self.created_by,
            guild_id: self.guild_id,
            max_uses: self.max_uses,
            uses: self.uses,
            expires_at: self.expires_at.map(Timestamp::new),
            revoked_at: self.revoked_at.map(Timestamp::new),
            created_at: Timestamp::new(self.created_at),
        }
    }
}

pub async fn insert(
    pool: &PgPool,
    id: Uuid,
    code: &str,
    created_by: Uuid,
    guild_id: Option<Uuid>,
    max_uses: i32,
    expires_at: Option<OffsetDateTime>,
) -> DbResult<InviteRow> {
    let row = sqlx::query_as!(
        InviteRow,
        r#"
        INSERT INTO invites (id, code, created_by, guild_id, max_uses, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, code, created_by, guild_id, max_uses, uses,
                  expires_at, revoked_at, created_at
        "#,
        id,
        code,
        created_by,
        guild_id,
        max_uses,
        expires_at,
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn find_by_code<'e, E: PgExecutor<'e>>(
    executor: E,
    code: &str,
) -> DbResult<Option<InviteRow>> {
    let row = sqlx::query_as!(
        InviteRow,
        r#"
        SELECT id, code, created_by, guild_id, max_uses, uses,
               expires_at, revoked_at, created_at
        FROM invites WHERE code = $1
        "#,
        code
    )
    .fetch_optional(executor)
    .await?;
    Ok(row)
}

/// Consumes one use, atomically. Returns `NotFound` when the code does not exist
/// or is no longer usable — the `WHERE` clause is the whole guard, so two
/// simultaneous registrations cannot both take the last use.
pub async fn consume<'e, E: PgExecutor<'e>>(executor: E, code: &str) -> DbResult<InviteRow> {
    missing(
        "invite",
        sqlx::query_as!(
            InviteRow,
            r#"
            UPDATE invites SET uses = uses + 1
            WHERE code = $1
              AND revoked_at IS NULL
              AND (expires_at IS NULL OR expires_at > NOW())
              AND uses < max_uses
            RETURNING id, code, created_by, guild_id, max_uses, uses,
                      expires_at, revoked_at, created_at
            "#,
            code
        )
        .fetch_one(executor)
        .await,
    )
}

pub async fn list(pool: &PgPool) -> DbResult<Vec<InviteRow>> {
    let rows = sqlx::query_as!(
        InviteRow,
        r#"
        SELECT id, code, created_by, guild_id, max_uses, uses,
               expires_at, revoked_at, created_at
        FROM invites ORDER BY id DESC
        "#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn revoke(pool: &PgPool, code: &str) -> DbResult<InviteRow> {
    missing(
        "invite",
        sqlx::query_as!(
            InviteRow,
            r#"
            UPDATE invites SET revoked_at = NOW()
            WHERE code = $1 AND revoked_at IS NULL
            RETURNING id, code, created_by, guild_id, max_uses, uses,
                      expires_at, revoked_at, created_at
            "#,
            code
        )
        .fetch_one(pool)
        .await,
    )
}
