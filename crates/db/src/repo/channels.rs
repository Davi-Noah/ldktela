//! `channels` and `channel_participants`.

use protocol::channel::{Channel, ChannelType as WireChannelType};
use protocol::scalars::{PermissionMask, Snowflake, Timestamp};
use protocol::user::UserSummary;
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{missing, DbResult};
use crate::types::ChannelType;

#[derive(Debug, Clone)]
pub struct ChannelRow {
    pub id: Uuid,
    pub guild_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub name: String,
    pub topic: Option<String>,
    pub kind: ChannelType,
    pub position: i32,
    pub discord_channel_id: Option<i64>,
    pub bridge_enabled: bool,
    pub created_at: OffsetDateTime,
}

impl ChannelRow {
    /// The wire object needs the requester's resolved mask, which only the
    /// caller knows, so it is passed in rather than guessed here.
    pub fn to_wire(&self, permissions: i64, participants: Option<Vec<UserSummary>>) -> Channel {
        Channel {
            id: self.id,
            guild_id: self.guild_id,
            category_id: self.category_id,
            name: self.name.clone(),
            topic: self.topic.clone(),
            kind: WireChannelType::from(self.kind),
            position: self.position,
            bridge_enabled: self.bridge_enabled,
            discord_channel_id: self.discord_channel_id.map(Snowflake::new),
            participants,
            permissions: PermissionMask::new(permissions),
            created_at: Timestamp::new(self.created_at),
        }
    }
}

pub async fn find_by_id<'e, E: PgExecutor<'e>>(executor: E, id: Uuid) -> DbResult<ChannelRow> {
    missing(
        "channel",
        sqlx::query_as!(
            ChannelRow,
            r#"
            SELECT id, guild_id, category_id, name, topic,
                   type AS "kind: ChannelType", position,
                   discord_channel_id, bridge_enabled, created_at
            FROM channels WHERE id = $1
            "#,
            id
        )
        .fetch_one(executor)
        .await,
    )
}

/// Every channel of a guild, in render order. Permission filtering happens in
/// the caller, per channel, at query time (CLAUDE.md §2.7).
pub async fn list_by_guild<'e, E: PgExecutor<'e>>(
    executor: E,
    guild_id: Uuid,
) -> DbResult<Vec<ChannelRow>> {
    let rows = sqlx::query_as!(
        ChannelRow,
        r#"
        SELECT id, guild_id, category_id, name, topic,
               type AS "kind: ChannelType", position,
               discord_channel_id, bridge_enabled, created_at
        FROM channels
        WHERE guild_id = $1
        ORDER BY position, id
        "#,
        guild_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

/// Direct conversations the user still participates in.
pub async fn list_direct_for_user<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
) -> DbResult<Vec<ChannelRow>> {
    let rows = sqlx::query_as!(
        ChannelRow,
        r#"
        SELECT c.id, c.guild_id, c.category_id, c.name, c.topic,
               c.type AS "kind: ChannelType", c.position,
               c.discord_channel_id, c.bridge_enabled, c.created_at
        FROM channels c
        JOIN channel_participants p ON p.channel_id = c.id
        WHERE p.user_id = $1 AND p.left_at IS NULL
        ORDER BY c.id DESC
        "#,
        user_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

/// Active participants of a direct conversation, as wire summaries.
pub async fn participants<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
) -> DbResult<Vec<UserSummary>> {
    let rows = sqlx::query!(
        r#"
        SELECT u.id, u.username, u.display_name, u.avatar_url, u.accent_color, u.is_migrated
        FROM channel_participants p
        JOIN users u ON u.id = p.user_id
        WHERE p.channel_id = $1 AND p.left_at IS NULL
        ORDER BY p.joined_at, u.id
        "#,
        channel_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| UserSummary {
            id: r.id,
            username: r.username,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
            accent_color: r.accent_color,
            is_migrated: r.is_migrated,
        })
        .collect())
}

/// Ids of the active participants. Cheaper than [`participants`] when the
/// gateway only needs a recipient set.
pub async fn participant_ids<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
) -> DbResult<Vec<Uuid>> {
    let ids = sqlx::query_scalar!(
        "SELECT user_id FROM channel_participants \
         WHERE channel_id = $1 AND left_at IS NULL",
        channel_id
    )
    .fetch_all(executor)
    .await?;
    Ok(ids)
}

/// The columns a new guild channel needs. A parameter object rather than eight
/// positional arguments, which is where a `guild_id`/`category_id` swap hides.
#[derive(Debug, Clone)]
pub struct NewGuildChannel<'a> {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub category_id: Option<Uuid>,
    pub name: &'a str,
    pub topic: Option<&'a str>,
    pub kind: ChannelType,
    pub position: i32,
}

pub async fn insert_guild_channel<'e, E: PgExecutor<'e>>(
    executor: E,
    channel: NewGuildChannel<'_>,
) -> DbResult<ChannelRow> {
    let row = sqlx::query_as!(
        ChannelRow,
        r#"
        INSERT INTO channels (id, guild_id, category_id, name, topic, type, position)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, guild_id, category_id, name, topic,
                  type AS "kind: ChannelType", position,
                  discord_channel_id, bridge_enabled, created_at
        "#,
        channel.id,
        channel.guild_id,
        channel.category_id,
        channel.name,
        channel.topic,
        channel.kind as ChannelType,
        channel.position,
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

pub async fn insert_direct_channel<'e, E: PgExecutor<'e>>(
    executor: E,
    id: Uuid,
    name: &str,
    kind: ChannelType,
) -> DbResult<ChannelRow> {
    let row = sqlx::query_as!(
        ChannelRow,
        r#"
        INSERT INTO channels (id, name, type)
        VALUES ($1, $2, $3)
        RETURNING id, guild_id, category_id, name, topic,
                  type AS "kind: ChannelType", position,
                  discord_channel_id, bridge_enabled, created_at
        "#,
        id,
        name,
        kind as ChannelType,
    )
    .fetch_one(executor)
    .await?;
    Ok(row)
}

pub async fn add_participant<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    user_id: Uuid,
    added_by: Option<Uuid>,
) -> DbResult<()> {
    sqlx::query!(
        "INSERT INTO channel_participants (channel_id, user_id, added_by) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (channel_id, user_id) DO UPDATE SET left_at = NULL",
        channel_id,
        user_id,
        added_by
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Leaving keeps the row: messages stay visible to the others, and the history
/// of who was in the conversation is not rewritten (RF-18b).
pub async fn remove_participant<'e, E: PgExecutor<'e>>(
    executor: E,
    channel_id: Uuid,
    user_id: Uuid,
) -> DbResult<bool> {
    let result = sqlx::query!(
        "UPDATE channel_participants SET left_at = NOW() \
         WHERE channel_id = $1 AND user_id = $2 AND left_at IS NULL",
        channel_id,
        user_id
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Finds the existing 1:1 conversation between exactly these two users, if any.
///
/// "Exactly" matters: a group that happens to contain both must not be returned
/// (RF-18). The `HAVING` clause is the whole guard.
pub async fn find_direct_between<'e, E: PgExecutor<'e>>(
    executor: E,
    a: Uuid,
    b: Uuid,
) -> DbResult<Option<Uuid>> {
    let id = sqlx::query_scalar!(
        r#"
        SELECT c.id
        FROM channels c
        JOIN channel_participants p ON p.channel_id = c.id
        WHERE c.type = 'dm'
        GROUP BY c.id
        HAVING COUNT(*) = 2
           AND bool_or(p.user_id = $1) AND bool_or(p.user_id = $2)
        LIMIT 1
        "#,
        a,
        b
    )
    .fetch_optional(executor)
    .await?;
    Ok(id)
}

/// Bridge scope is enforced twice: here, and by `chk_bridge_scope` in the schema.
pub async fn set_bridge_enabled(
    pool: &PgPool,
    channel_id: Uuid,
    enabled: bool,
    discord_channel_id: Option<i64>,
) -> DbResult<ChannelRow> {
    missing(
        "channel",
        sqlx::query_as!(
            ChannelRow,
            r#"
            UPDATE channels
            SET bridge_enabled = $2,
                discord_channel_id = COALESCE($3, discord_channel_id)
            WHERE id = $1
            RETURNING id, guild_id, category_id, name, topic,
                      type AS "kind: ChannelType", position,
                      discord_channel_id, bridge_enabled, created_at
            "#,
            channel_id,
            enabled,
            discord_channel_id,
        )
        .fetch_one(pool)
        .await,
    )
}
