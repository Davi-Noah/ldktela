//! Route-level permission guards.
//!
//! Every guard answers two questions in this order, and never collapses them:
//!
//! 1. **Can the caller see the resource at all?** No → `404`, never `403`.
//!    A `403` confirms the resource exists and who is in it, which is enough to
//!    map a private server (`docs/api/rest-api.md` §3).
//! 2. **May the caller perform this action?** No → `403`.
//!
//! The resolution itself always comes from `db::repo::permissions`, which reads
//! the database at query time. The gateway's routing cache is never consulted
//! here (CLAUDE.md §2.7).

use db::repo::channels::ChannelRow;
use db::repo::guilds::GuildRow;
use db::repo::{channels, guilds, permissions};
use domain::Permissions;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

/// A channel the caller can see, with their resolved mask.
pub struct ChannelAccess {
    pub channel: ChannelRow,
    pub permissions: Permissions,
}

/// A guild the caller belongs to, with their guild-level mask.
pub struct GuildAccess {
    pub guild: GuildRow,
    pub permissions: Permissions,
}

/// `404` unless the channel exists **and** the caller has `VIEW_CHANNEL` on it.
pub async fn channel_visible(
    state: &AppState,
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<ChannelAccess, AppError> {
    let Some(mask) = permissions::resolve_for_channel(&state.pool, user_id, channel_id).await?
    else {
        return Err(AppError::invisible("channel"));
    };
    if !mask.contains(Permissions::VIEW_CHANNEL) {
        return Err(AppError::invisible("channel"));
    }
    let channel = channels::find_by_id(&state.pool, channel_id).await?;
    Ok(ChannelAccess {
        channel,
        permissions: mask,
    })
}

/// `404` when invisible, `403` when visible but not permitted.
pub async fn require_channel(
    state: &AppState,
    user_id: Uuid,
    channel_id: Uuid,
    required: Permissions,
) -> Result<ChannelAccess, AppError> {
    let access = channel_visible(state, user_id, channel_id).await?;
    if !access.permissions.contains(required) {
        return Err(AppError::Forbidden);
    }
    Ok(access)
}

/// `404` unless the guild exists and the caller is an unbanned member.
///
/// Membership, not `VIEW_CHANNEL`, is the visibility gate for guild-scoped
/// routes: a member with every channel denied still needs to read the role list
/// to understand why.
pub async fn guild_visible(
    state: &AppState,
    user_id: Uuid,
    guild_id: Uuid,
) -> Result<GuildAccess, AppError> {
    let guild = guilds::find_by_id(&state.pool, guild_id)
        .await
        .map_err(|_| AppError::invisible("guild"))?;
    if guild.owner_id != user_id && !guilds::is_member(&state.pool, guild_id, user_id).await? {
        return Err(AppError::invisible("guild"));
    }
    let mask = permissions::resolve_for_guild(&state.pool, user_id, guild_id).await?;
    Ok(GuildAccess {
        guild,
        permissions: mask,
    })
}

/// `404` when invisible, `403` when a member without the permission.
pub async fn require_guild(
    state: &AppState,
    user_id: Uuid,
    guild_id: Uuid,
    required: Permissions,
) -> Result<GuildAccess, AppError> {
    let access = guild_visible(state, user_id, guild_id).await?;
    if !access.permissions.contains(required) {
        return Err(AppError::Forbidden);
    }
    Ok(access)
}

/// The subset of a guild's channels the caller can see, each with its resolved
/// mask. Recomputed per request; never read from the gateway index.
pub async fn visible_channels(
    state: &AppState,
    user_id: Uuid,
    guild_id: Uuid,
) -> Result<Vec<ChannelAccess>, AppError> {
    let mut visible = Vec::new();
    for channel in channels::list_by_guild(&state.pool, guild_id).await? {
        let Some(mask) = permissions::resolve_for_channel(&state.pool, user_id, channel.id).await?
        else {
            continue;
        };
        if mask.contains(Permissions::VIEW_CHANNEL) {
            visible.push(ChannelAccess {
                channel,
                permissions: mask,
            });
        }
    }
    Ok(visible)
}
