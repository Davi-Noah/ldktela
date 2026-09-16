//! `live_tags` (RF-38 to RF-40).
//!
//! Remembers the nickname a member had **before** we prefixed it with `[LIVE] `,
//! so it can be put back exactly — including the common case of there having
//! been no nickname at all, which has to go back to no nickname rather than
//! becoming the username frozen in place.
//!
//! This is the one piece of Discord state the product writes into someone else's
//! server, and getting the restore wrong damages something that is not ours
//! (ADR-0024). That is why it is a real table and not a map in memory: the crash
//! that RF-40 exists to clean up after is exactly the event that would erase a
//! map in memory.

use sqlx::{PgExecutor, PgPool};
use time::OffsetDateTime;

use crate::error::DbResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTag {
    pub discord_guild_id: i64,
    pub discord_user_id: i64,
    /// `None` means the member had no nickname before being tagged.
    pub previous_nick: Option<String>,
}

/// Records that a member was tagged, keeping whatever nickname they had.
///
/// Idempotent by design (ADR-0024, guard 4): tagging someone already tagged must
/// not overwrite the remembered nickname with the tagged one, which would make
/// the prefix permanent.
pub async fn remember<'e, E: PgExecutor<'e>>(
    executor: E,
    discord_guild_id: i64,
    discord_user_id: i64,
    previous_nick: Option<&str>,
    at: OffsetDateTime,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO live_tags (discord_guild_id, discord_user_id, previous_nick, tagged_at)
             VALUES ($1, $2, $3, $4)
        ON CONFLICT (discord_guild_id, discord_user_id) DO NOTHING
        "#,
        discord_guild_id,
        discord_user_id,
        previous_nick,
        at,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Takes the remembered nickname back, clearing the record.
///
/// Returns `None` when the member was not tagged, which is how "untag someone
/// who is not tagged does nothing" stays free.
pub async fn forget(
    pool: &PgPool,
    discord_guild_id: i64,
    discord_user_id: i64,
) -> DbResult<Option<LiveTag>> {
    let row = sqlx::query!(
        r#"
        DELETE FROM live_tags
              WHERE discord_guild_id = $1 AND discord_user_id = $2
          RETURNING discord_guild_id, discord_user_id, previous_nick
        "#,
        discord_guild_id,
        discord_user_id,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| LiveTag {
        discord_guild_id: row.discord_guild_id,
        discord_user_id: row.discord_user_id,
        previous_nick: row.previous_nick,
    }))
}

/// Everyone still marked, for the startup sweep (RF-40).
pub async fn all(pool: &PgPool) -> DbResult<Vec<LiveTag>> {
    let rows = sqlx::query!(
        r#"
        SELECT discord_guild_id, discord_user_id, previous_nick
          FROM live_tags
         ORDER BY tagged_at
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| LiveTag {
            discord_guild_id: row.discord_guild_id,
            discord_user_id: row.discord_user_id,
            previous_nick: row.previous_nick,
        })
        .collect())
}
