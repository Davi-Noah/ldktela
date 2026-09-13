//! `room_presence` (RF-12).
//!
//! Who is in which room right now. The table is `UNLOGGED`: without a socket
//! there is no presence, so none of this should survive a crash.

use protocol::room::RoomParticipant;
use protocol::scalars::Snowflake;
use protocol::user::UserSummary;
use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::DbResult;

/// One participant, joined with the profile needed to render them.
#[derive(Debug, Clone)]
pub struct ParticipantRow {
    pub user_id: Uuid,
    pub discord_user_id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub publishing: bool,
    pub joined_at: OffsetDateTime,
}

impl ParticipantRow {
    pub fn to_wire(&self) -> RoomParticipant {
        RoomParticipant {
            user: UserSummary {
                id: self.user_id,
                discord_user_id: Snowflake::new(self.discord_user_id),
                username: self.username.clone(),
                display_name: self.display_name.clone(),
                avatar_url: self.avatar_url.clone(),
            },
            publishing: self.publishing,
        }
    }
}

/// Put a user in a room, or move them to a different one.
///
/// A user is in at most one room, because they are in at most one Discord voice
/// channel — hence the primary key on `user_id` and the upsert here.
pub async fn join<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
    discord_channel_id: i64,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO room_presence (user_id, discord_channel_id)
        VALUES ($1, $2)
        ON CONFLICT (user_id) DO UPDATE
           SET discord_channel_id = EXCLUDED.discord_channel_id,
               publishing         = FALSE,
               joined_at          = NOW()
        "#,
        user_id,
        discord_channel_id,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Remove a user from whatever room they were in. Returns the channel they left,
/// so the caller knows who to notify without a second query.
pub async fn leave<'e, E: PgExecutor<'e>>(executor: E, user_id: Uuid) -> DbResult<Option<i64>> {
    let channel = sqlx::query_scalar!(
        r#"DELETE FROM room_presence WHERE user_id = $1 RETURNING discord_channel_id"#,
        user_id,
    )
    .fetch_optional(executor)
    .await?;
    Ok(channel)
}

/// Flip the publishing flag. Returns the channel, or `None` if the user is not
/// in a room — which happens when a LiveKit webhook arrives after the user
/// already left, and is not an error.
pub async fn set_publishing<'e, E: PgExecutor<'e>>(
    executor: E,
    user_id: Uuid,
    publishing: bool,
) -> DbResult<Option<i64>> {
    let channel = sqlx::query_scalar!(
        r#"
        UPDATE room_presence
           SET publishing = $2
         WHERE user_id = $1
        RETURNING discord_channel_id
        "#,
        user_id,
        publishing,
    )
    .fetch_optional(executor)
    .await?;
    Ok(channel)
}

pub async fn list_by_channel(
    pool: &PgPool,
    discord_channel_id: i64,
) -> DbResult<Vec<ParticipantRow>> {
    let rows = sqlx::query_as!(
        ParticipantRow,
        r#"
        SELECT p.user_id,
               u.discord_user_id,
               u.username,
               u.display_name,
               u.avatar_url,
               p.publishing,
               p.joined_at
          FROM room_presence p
          JOIN users u ON u.id = p.user_id
         WHERE p.discord_channel_id = $1
         ORDER BY p.joined_at
        "#,
        discord_channel_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Which room a user is in, if any.
pub async fn channel_of(pool: &PgPool, user_id: Uuid) -> DbResult<Option<i64>> {
    let channel = sqlx::query_scalar!(
        r#"SELECT discord_channel_id FROM room_presence WHERE user_id = $1"#,
        user_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(channel)
}

/// How many people are publishing in a room. Feeds the admission guard.
pub async fn publisher_count(pool: &PgPool, discord_channel_id: i64) -> DbResult<i64> {
    let count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
          FROM room_presence
         WHERE discord_channel_id = $1 AND publishing
        "#,
        discord_channel_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Every room that currently has someone in it.
///
/// Feeds the revocation sweep: after a role changes, only occupied rooms are
/// worth rechecking, and there are rarely more than a handful.
pub async fn occupied_channels(pool: &PgPool) -> DbResult<Vec<i64>> {
    let rows = sqlx::query_scalar!(r#"SELECT DISTINCT discord_channel_id FROM room_presence"#)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Empty a room. Used when LiveKit reports the room finished.
pub async fn clear_channel(pool: &PgPool, discord_channel_id: i64) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"DELETE FROM room_presence WHERE discord_channel_id = $1"#,
        discord_channel_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
