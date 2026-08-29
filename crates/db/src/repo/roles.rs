//! `roles` and `channel_overwrites` (RF-07, RF-07a).

use protocol::guild::{ChannelOverwrite, OverwriteTarget as WireOverwriteTarget, Role};
use protocol::scalars::{PermissionMask, Timestamp};
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{missing, DbResult};
use crate::types::OverwriteTarget;

#[derive(Debug, Clone)]
pub struct RoleRow {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub position: i32,
    pub permissions: i64,
    pub is_default: bool,
    pub hoist: bool,
    pub created_at: OffsetDateTime,
}

impl RoleRow {
    pub fn to_wire(&self) -> Role {
        Role {
            id: self.id,
            guild_id: self.guild_id,
            name: self.name.clone(),
            color: self.color.clone(),
            position: self.position,
            permissions: PermissionMask::new(self.permissions),
            is_default: self.is_default,
            hoist: self.hoist,
            created_at: Timestamp::new(self.created_at),
        }
    }
}

/// Creates the implicit `@everyone` role. `idx_roles_default` guarantees there
/// is at most one per guild.
pub async fn insert_default<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    guild_id: Uuid,
    permissions: i64,
) -> DbResult<RoleRow> {
    let row = sqlx::query_as!(
        RoleRow,
        r#"
        INSERT INTO roles (id, guild_id, name, permissions, is_default, position)
        VALUES ($1, $2, '@everyone', $3, TRUE, 0)
        RETURNING id, guild_id, name, color, position, permissions,
                  is_default, hoist, created_at
        "#,
        id,
        guild_id,
        permissions
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

/// The columns a new role needs. A parameter object rather than eight
/// positional arguments, where a `position`/`permissions` swap would silently
/// grant the wrong mask.
#[derive(Debug, Clone)]
pub struct NewRole<'a> {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: &'a str,
    pub color: Option<&'a str>,
    pub position: i32,
    pub permissions: i64,
    pub hoist: bool,
}

pub async fn insert(pool: &PgPool, role: NewRole<'_>) -> DbResult<RoleRow> {
    let NewRole {
        id,
        guild_id,
        name,
        color,
        position,
        permissions,
        hoist,
    } = role;
    let row = sqlx::query_as!(
        RoleRow,
        r#"
        INSERT INTO roles (id, guild_id, name, color, position, permissions, hoist)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, guild_id, name, color, position, permissions,
                  is_default, hoist, created_at
        "#,
        id,
        guild_id,
        name,
        color,
        position,
        permissions,
        hoist
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn find_by_id<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
    id: Uuid,
) -> DbResult<RoleRow> {
    missing(
        "role",
        sqlx::query_as!(
            RoleRow,
            "SELECT id, guild_id, name, color, position, permissions, \
                    is_default, hoist, created_at \
             FROM roles WHERE guild_id = $1 AND id = $2",
            guild_id,
            id
        )
        .fetch_one(executor)
        .await,
    )
}

/// Highest position first, matching how a role list is rendered.
pub async fn list_by_guild<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
) -> DbResult<Vec<RoleRow>> {
    let rows = sqlx::query_as!(
        RoleRow,
        "SELECT id, guild_id, name, color, position, permissions, \
                is_default, hoist, created_at \
         FROM roles WHERE guild_id = $1 ORDER BY position DESC, id",
        guild_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
pub async fn update(
    pool: &PgPool,
    guild_id: Uuid,
    id: Uuid,
    name: Option<String>,
    color: Option<Option<String>>,
    position: Option<i32>,
    permissions: Option<i64>,
    hoist: Option<bool>,
) -> DbResult<RoleRow> {
    missing(
        "role",
        sqlx::query_as!(
            RoleRow,
            r#"
            UPDATE roles SET
                name        = COALESCE($3, name),
                color       = CASE WHEN $4 THEN $5 ELSE color END,
                position    = COALESCE($6, position),
                permissions = COALESCE($7, permissions),
                hoist       = COALESCE($8, hoist)
            WHERE guild_id = $1 AND id = $2
            RETURNING id, guild_id, name, color, position, permissions,
                      is_default, hoist, created_at
            "#,
            guild_id,
            id,
            name,
            color.is_some(),
            color.flatten(),
            position,
            permissions,
            hoist,
        )
        .fetch_one(pool)
        .await,
    )
}

/// Deleting `@everyone` would leave the guild with no base mask, so the guard is
/// in the `WHERE`: the row simply does not match.
pub async fn delete(pool: &PgPool, guild_id: Uuid, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        "DELETE FROM roles WHERE guild_id = $1 AND id = $2 AND is_default = FALSE",
        guild_id,
        id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

// ---------------------------------------------------------------------------
// Overwrites de canal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OverwriteRow {
    pub channel_id: Uuid,
    pub target_type: OverwriteTarget,
    pub target_id: Uuid,
    pub allow: i64,
    pub deny: i64,
}

impl OverwriteRow {
    pub fn to_wire(&self) -> ChannelOverwrite {
        ChannelOverwrite {
            channel_id: self.channel_id,
            target_type: WireOverwriteTarget::from(self.target_type),
            target_id: self.target_id,
            allow: PermissionMask::new(self.allow),
            deny: PermissionMask::new(self.deny),
        }
    }
}

pub async fn put_overwrite(
    pool: &PgPool,
    channel_id: Uuid,
    target_type: OverwriteTarget,
    target_id: Uuid,
    allow: i64,
    deny: i64,
) -> DbResult<OverwriteRow> {
    let row = sqlx::query_as!(
        OverwriteRow,
        r#"
        INSERT INTO channel_overwrites (channel_id, target_type, target_id, allow, deny)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (channel_id, target_type, target_id)
        DO UPDATE SET allow = EXCLUDED.allow, deny = EXCLUDED.deny
        RETURNING channel_id, target_type AS "target_type: OverwriteTarget",
                  target_id, allow, deny
        "#,
        channel_id,
        target_type as OverwriteTarget,
        target_id,
        allow,
        deny
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_overwrite(
    pool: &PgPool,
    channel_id: Uuid,
    target_type: OverwriteTarget,
    target_id: Uuid,
) -> DbResult<bool> {
    let result = sqlx::query!(
        "DELETE FROM channel_overwrites \
         WHERE channel_id = $1 AND target_type = $2 AND target_id = $3",
        channel_id,
        target_type as OverwriteTarget,
        target_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn list_overwrites<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
) -> DbResult<Vec<OverwriteRow>> {
    let rows = sqlx::query_as!(
        OverwriteRow,
        r#"
        SELECT channel_id, target_type AS "target_type: OverwriteTarget",
               target_id, allow, deny
        FROM channel_overwrites WHERE channel_id = $1
        "#,
        channel_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}
