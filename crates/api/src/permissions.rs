//! Route-level authorization (ADR-0010).
//!
//! Every guard answers the questions in this order, and never collapses them:
//!
//! 1. **Can we still vouch for anything?** The replica went stale → `503`. Fail
//!    closed; a stale yes is worse than a temporary no (RF-09).
//! 2. **Does the caller see the channel at all?** No → `404`, never `403`.
//!    A `403` would confirm that a channel exists and that the caller is
//!    adjacent to it, which is enough to map a private server.
//! 3. **May the caller do this?** No → `403`. At this point the channel is
//!    already visible to them in Discord, so there is nothing left to leak.
//!
//! The answer always comes from the replica, and the replica is fed by Discord's
//! gateway. Nothing here consults a cache of our own.

use db::repo::users::{self, UserRow};
use domain::DiscordPermissions;
use uuid::Uuid;

use crate::discord::Staleness;
use crate::error::AppError;
use crate::state::AppState;

/// A room the caller may be in, with everything the handlers need after.
pub struct RoomAccess {
    pub user: UserRow,
    pub permissions: DiscordPermissions,
    pub discord_guild_id: i64,
    pub channel_name: String,
}

/// `503` when stale, `404` when invisible, `403` when visible but closed.
pub async fn room_access(
    state: &AppState,
    user_id: Uuid,
    discord_channel_id: i64,
) -> Result<RoomAccess, AppError> {
    if state.replica.staleness().await == Staleness::Stale {
        return Err(AppError::ReplicaStale);
    }

    let user = users::find_by_id(&state.pool, user_id).await?;

    // Um id negativo nunca e um snowflake; recusar aqui evita converter para
    // u64 e consultar a replica com um numero absurdo.
    let channel = u64::try_from(discord_channel_id).map_err(|_| AppError::invisible("room"))?;
    let discord_user = u64::try_from(user.discord_user_id)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("discord_user_id negativo no banco")))?;

    // `None` cobre canal desconhecido e nao-membro do guild. Os dois viram 404
    // pelo mesmo motivo: o solicitante nao deve conseguir distinguir "nao
    // existe" de "existe e nao e para voce".
    let Some(permissions) = state.replica.permissions(discord_user, channel).await else {
        return Err(AppError::invisible("room"));
    };
    if !permissions.can_view() {
        return Err(AppError::invisible("room"));
    }
    if !permissions.can_join_room() {
        return Err(AppError::Forbidden);
    }

    let (discord_guild_id, channel_name) = state
        .replica
        .channel_info(channel)
        .await
        .ok_or_else(|| AppError::invisible("room"))?;

    Ok(RoomAccess {
        user,
        permissions,
        discord_guild_id: i64::try_from(discord_guild_id).unwrap_or_default(),
        channel_name,
    })
}

/// The same guard, plus Discord's own "Video" right (ADR-0010): if the guild
/// says you may not go live in this channel, neither do we.
pub async fn publish_access(
    state: &AppState,
    user_id: Uuid,
    discord_channel_id: i64,
) -> Result<RoomAccess, AppError> {
    let access = room_access(state, user_id, discord_channel_id).await?;
    if !access.permissions.can_publish() {
        return Err(AppError::Forbidden);
    }
    Ok(access)
}
