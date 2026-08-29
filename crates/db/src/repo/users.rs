//! `users` (RF-01, RF-03, RF-26).

use protocol::scalars::{Snowflake, Timestamp};
use protocol::user::{CurrentUser, PresenceStatus, User, UserSummary};
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{missing, DbResult};

/// One row of `users`.
#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: Uuid,
    pub email: Option<String>,
    pub username: String,
    pub display_name: Option<String>,
    pub password_hash: Option<String>,
    pub avatar_url: Option<String>,
    pub accent_color: Option<String>,
    pub bio: Option<String>,
    pub discord_user_id: Option<i64>,
    pub is_migrated: bool,
    pub created_at: OffsetDateTime,
}

impl UserRow {
    pub fn to_summary(&self) -> UserSummary {
        UserSummary {
            id: self.id,
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            avatar_url: self.avatar_url.clone(),
            accent_color: self.accent_color.clone(),
            is_migrated: self.is_migrated,
        }
    }

    pub fn to_public(&self) -> User {
        User {
            id: self.id,
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            avatar_url: self.avatar_url.clone(),
            accent_color: self.accent_color.clone(),
            bio: self.bio.clone(),
            is_migrated: self.is_migrated,
            created_at: Timestamp::new(self.created_at),
        }
    }

    /// `status` is not persisted: presence lives in the gateway, in memory.
    pub fn to_current(&self, status: PresenceStatus) -> CurrentUser {
        CurrentUser {
            id: self.id,
            email: self.email.clone(),
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            avatar_url: self.avatar_url.clone(),
            accent_color: self.accent_color.clone(),
            bio: self.bio.clone(),
            discord_user_id: self.discord_user_id.map(Snowflake::new),
            status,
            created_at: Timestamp::new(self.created_at),
        }
    }
}

/// Creates a real account. Ghost users go through [`insert_ghost`].
///
/// Takes an executor so the caller can run it inside the registration
/// transaction that also consumes the invite.
pub async fn insert_real<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    email: &str,
    username: &str,
    password_hash: &str,
) -> DbResult<UserRow> {
    let row = sqlx::query_as!(
        UserRow,
        r#"
        INSERT INTO users (id, email, username, password_hash, is_migrated)
        VALUES ($1, $2, $3, $4, FALSE)
        RETURNING id, email, username, display_name, password_hash, avatar_url,
                  accent_color, bio, discord_user_id, is_migrated, created_at
        "#,
        id,
        email,
        username,
        password_hash,
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

/// Creates a ghost user for an imported Discord author (RF-26): no email, no
/// password, `discord_user_id` set.
pub async fn insert_ghost<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    username: &str,
    display_name: Option<&str>,
    discord_user_id: i64,
    avatar_url: Option<&str>,
) -> DbResult<UserRow> {
    let row = sqlx::query_as!(
        UserRow,
        r#"
        INSERT INTO users (id, username, display_name, discord_user_id, avatar_url, is_migrated)
        VALUES ($1, $2, $3, $4, $5, TRUE)
        RETURNING id, email, username, display_name, password_hash, avatar_url,
                  accent_color, bio, discord_user_id, is_migrated, created_at
        "#,
        id,
        username,
        display_name,
        discord_user_id,
        avatar_url,
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

pub async fn find_by_id<'e, E: PgExecutor<'e>>(executor: E, id: Uuid) -> DbResult<UserRow> {
    missing(
        "user",
        sqlx::query_as!(
            UserRow,
            r#"
            SELECT id, email, username, display_name, password_hash, avatar_url,
                   accent_color, bio, discord_user_id, is_migrated, created_at
            FROM users WHERE id = $1
            "#,
            id
        )
        .fetch_one(executor)
        .await,
    )
}

/// Case-insensitive, matching `idx_users_email`.
pub async fn find_by_email(pool: &PgPool, email: &str) -> DbResult<Option<UserRow>> {
    let row = sqlx::query_as!(
        UserRow,
        r#"
        SELECT id, email, username, display_name, password_hash, avatar_url,
               accent_color, bio, discord_user_id, is_migrated, created_at
        FROM users WHERE lower(email) = lower($1)
        "#,
        email
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Case-insensitive, and only over real accounts: `idx_users_username` is
/// partial on `is_migrated = FALSE`, so two ghosts may share a username.
pub async fn find_real_by_username<'e, E: PgExecutor<'e>>(
    executor: E,
    username: &str,
) -> DbResult<Option<UserRow>> {
    let row = sqlx::query_as!(
        UserRow,
        r#"
        SELECT id, email, username, display_name, password_hash, avatar_url,
               accent_color, bio, discord_user_id, is_migrated, created_at
        FROM users WHERE lower(username) = lower($1) AND is_migrated = FALSE
        "#,
        username
    )
    .fetch_optional(executor)
    .await?;
    Ok(row)
}

pub async fn find_by_discord_id<'e, E: PgExecutor<'e>>(
    executor: E,
    discord_user_id: i64,
) -> DbResult<Option<UserRow>> {
    let row = sqlx::query_as!(
        UserRow,
        r#"
        SELECT id, email, username, display_name, password_hash, avatar_url,
               accent_color, bio, discord_user_id, is_migrated, created_at
        FROM users WHERE discord_user_id = $1
        "#,
        discord_user_id
    )
    .fetch_optional(executor)
    .await?;
    Ok(row)
}

/// Applies a `PATCH /users/@me`. `None` leaves the column alone; `Some(None)`
/// clears it (`docs/api/rest-api.md` §1).
#[allow(clippy::type_complexity)]
pub async fn update_profile(
    pool: &PgPool,
    id: Uuid,
    display_name: Option<Option<String>>,
    avatar_url: Option<Option<String>>,
    bio: Option<Option<String>>,
    accent_color: Option<Option<String>>,
) -> DbResult<UserRow> {
    missing(
        "user",
        sqlx::query_as!(
            UserRow,
            r#"
            UPDATE users SET
                display_name = CASE WHEN $2 THEN $3 ELSE display_name END,
                avatar_url   = CASE WHEN $4 THEN $5 ELSE avatar_url   END,
                bio          = CASE WHEN $6 THEN $7 ELSE bio          END,
                accent_color = CASE WHEN $8 THEN $9 ELSE accent_color END,
                updated_at   = NOW()
            WHERE id = $1
            RETURNING id, email, username, display_name, password_hash, avatar_url,
                      accent_color, bio, discord_user_id, is_migrated, created_at
            "#,
            id,
            display_name.is_some(),
            display_name.flatten(),
            avatar_url.is_some(),
            avatar_url.flatten(),
            bio.is_some(),
            bio.flatten(),
            accent_color.is_some(),
            accent_color.flatten(),
        )
        .fetch_one(pool)
        .await,
    )
}
