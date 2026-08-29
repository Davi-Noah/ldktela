//! `guilds`, `guild_members` and `member_roles` (RF-05, RF-07b).

use protocol::guild::{Guild, Member};
use protocol::scalars::{Snowflake, Timestamp};
use protocol::user::UserSummary;
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{missing, DbResult};

#[derive(Debug, Clone)]
pub struct GuildRow {
    pub id: Uuid,
    pub name: String,
    pub icon_url: Option<String>,
    pub owner_id: Uuid,
    pub discord_guild_id: Option<i64>,
    pub created_at: OffsetDateTime,
}

impl GuildRow {
    pub fn to_wire(&self) -> Guild {
        Guild {
            id: self.id,
            name: self.name.clone(),
            icon_url: self.icon_url.clone(),
            owner_id: self.owner_id,
            discord_guild_id: self.discord_guild_id.map(Snowflake::new),
            created_at: Timestamp::new(self.created_at),
        }
    }
}

/// A member joined with their user row and role ids.
#[derive(Debug, Clone)]
pub struct MemberRow {
    pub guild_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub accent_color: Option<String>,
    pub is_migrated: bool,
    pub nickname: Option<String>,
    pub joined_at: OffsetDateTime,
    pub banned_at: Option<OffsetDateTime>,
    pub roles: Vec<Uuid>,
}

impl MemberRow {
    pub fn to_wire(&self) -> Member {
        Member {
            user: UserSummary {
                id: self.user_id,
                username: self.username.clone(),
                display_name: self.display_name.clone(),
                avatar_url: self.avatar_url.clone(),
                accent_color: self.accent_color.clone(),
                is_migrated: self.is_migrated,
            },
            guild_id: self.guild_id,
            nickname: self.nickname.clone(),
            roles: self.roles.clone(),
            joined_at: Timestamp::new(self.joined_at),
            banned_at: self.banned_at.map(Timestamp::new),
        }
    }
}

pub async fn insert<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    name: &str,
    owner_id: Uuid,
) -> DbResult<GuildRow> {
    let row = sqlx::query_as!(
        GuildRow,
        r#"
        INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)
        RETURNING id, name, icon_url, owner_id, discord_guild_id, created_at
        "#,
        id,
        name,
        owner_id
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

pub async fn find_by_id<'e, E: PgExecutor<'e>>(executor: E, id: Uuid) -> DbResult<GuildRow> {
    missing(
        "guild",
        sqlx::query_as!(
            GuildRow,
            "SELECT id, name, icon_url, owner_id, discord_guild_id, created_at \
             FROM guilds WHERE id = $1",
            id
        )
        .fetch_one(executor)
        .await,
    )
}

/// Guilds the user is an unbanned member of.
pub async fn list_for_user<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
) -> DbResult<Vec<GuildRow>> {
    let rows = sqlx::query_as!(
        GuildRow,
        r#"
        SELECT g.id, g.name, g.icon_url, g.owner_id, g.discord_guild_id, g.created_at
        FROM guilds g
        JOIN guild_members m ON m.guild_id = g.id
        WHERE m.user_id = $1 AND m.banned_at IS NULL
        ORDER BY g.id
        "#,
        user_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

pub async fn update(
    pool: &PgPool,
    id: Uuid,
    name: Option<Option<String>>,
    icon_url: Option<Option<String>>,
) -> DbResult<GuildRow> {
    missing(
        "guild",
        sqlx::query_as!(
            GuildRow,
            r#"
            UPDATE guilds SET
                name     = CASE WHEN $2 THEN COALESCE($3, name) ELSE name END,
                icon_url = CASE WHEN $4 THEN $5 ELSE icon_url END
            WHERE id = $1
            RETURNING id, name, icon_url, owner_id, discord_guild_id, created_at
            "#,
            id,
            name.is_some(),
            name.flatten(),
            icon_url.is_some(),
            icon_url.flatten(),
        )
        .fetch_one(pool)
        .await,
    )
}

pub async fn add_member<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
    user_id: Uuid,
) -> DbResult<()> {
    sqlx::query!(
        "INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2) \
         ON CONFLICT (guild_id, user_id) DO UPDATE SET banned_at = NULL",
        guild_id,
        user_id
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Kick: the row goes away, so the person can be invited back.
pub async fn remove_member(pool: &PgPool, guild_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        "DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2",
        guild_id,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Ban: the row stays with `banned_at` set, so the ban survives a re-invite.
pub async fn ban_member(pool: &PgPool, guild_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        "INSERT INTO guild_members (guild_id, user_id, banned_at) VALUES ($1, $2, NOW()) \
         ON CONFLICT (guild_id, user_id) DO UPDATE SET banned_at = NOW()",
        guild_id,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn is_member<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
    user_id: Uuid,
) -> DbResult<bool> {
    let found = sqlx::query_scalar!(
        "SELECT 1 FROM guild_members \
         WHERE guild_id = $1 AND user_id = $2 AND banned_at IS NULL",
        guild_id,
        user_id
    )
    .fetch_optional(executor)
    .await?;
    Ok(found.is_some())
}

pub async fn member_count<'e, E: PgExecutor<'e>>(executor: E, guild_id: Uuid) -> DbResult<i64> {
    let count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!" FROM guild_members
        WHERE guild_id = $1 AND banned_at IS NULL
        "#,
        guild_id
    )
    .fetch_one(executor)
    .await?;
    Ok(count)
}

pub async fn find_member<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<MemberRow>> {
    let row = sqlx::query!(
        r#"
        SELECT m.guild_id, m.user_id, u.username, u.display_name, u.avatar_url,
               u.accent_color, u.is_migrated, m.nickname, m.joined_at, m.banned_at,
               COALESCE(
                   ARRAY_AGG(mr.role_id) FILTER (WHERE mr.role_id IS NOT NULL),
                   '{}'
               ) AS "roles!: Vec<Uuid>"
        FROM guild_members m
        JOIN users u ON u.id = m.user_id
        LEFT JOIN member_roles mr ON mr.guild_id = m.guild_id AND mr.user_id = m.user_id
        WHERE m.guild_id = $1 AND m.user_id = $2
        GROUP BY m.guild_id, m.user_id, u.username, u.display_name, u.avatar_url,
                 u.accent_color, u.is_migrated, m.nickname, m.joined_at, m.banned_at
        "#,
        guild_id,
        user_id
    )
    .fetch_optional(executor)
    .await?;
    Ok(row.map(|r| MemberRow {
        guild_id: r.guild_id,
        user_id: r.user_id,
        username: r.username,
        display_name: r.display_name,
        avatar_url: r.avatar_url,
        accent_color: r.accent_color,
        is_migrated: r.is_migrated,
        nickname: r.nickname,
        joined_at: r.joined_at,
        banned_at: r.banned_at,
        roles: r.roles,
    }))
}

/// Unbanned members, with their role ids. At 10 to 30 users this is one query
/// and no pagination.
pub async fn list_members<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
) -> DbResult<Vec<MemberRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT m.guild_id, m.user_id, u.username, u.display_name, u.avatar_url,
               u.accent_color, u.is_migrated, m.nickname, m.joined_at, m.banned_at,
               COALESCE(
                   ARRAY_AGG(mr.role_id) FILTER (WHERE mr.role_id IS NOT NULL),
                   '{}'
               ) AS "roles!: Vec<Uuid>"
        FROM guild_members m
        JOIN users u ON u.id = m.user_id
        LEFT JOIN member_roles mr ON mr.guild_id = m.guild_id AND mr.user_id = m.user_id
        WHERE m.guild_id = $1 AND m.banned_at IS NULL
        GROUP BY m.guild_id, m.user_id, u.username, u.display_name, u.avatar_url,
                 u.accent_color, u.is_migrated, m.nickname, m.joined_at, m.banned_at
        ORDER BY lower(u.username)
        "#,
        guild_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| MemberRow {
            guild_id: r.guild_id,
            user_id: r.user_id,
            username: r.username,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
            accent_color: r.accent_color,
            is_migrated: r.is_migrated,
            nickname: r.nickname,
            joined_at: r.joined_at,
            banned_at: r.banned_at,
            roles: r.roles,
        })
        .collect())
}

pub async fn set_nickname(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    nickname: Option<&str>,
) -> DbResult<()> {
    let result = sqlx::query!(
        "UPDATE guild_members SET nickname = $3 WHERE guild_id = $1 AND user_id = $2",
        guild_id,
        user_id,
        nickname
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(crate::DbError::NotFound("member"));
    }
    Ok(())
}

/// Replaces the member's role set in one transaction: a partial apply would
/// leave someone with half of a promotion.
pub async fn replace_roles(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    roles: &[Uuid],
) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "DELETE FROM member_roles WHERE guild_id = $1 AND user_id = $2",
        guild_id,
        user_id
    )
    .execute(&mut *tx)
    .await?;
    for role in roles {
        sqlx::query!(
            "INSERT INTO member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)",
            guild_id,
            user_id,
            role
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
