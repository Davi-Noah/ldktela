//! Live revocation (RF-08).
//!
//! Permission checked only at the door is permission that leaks for the whole
//! length of the session, and a screen share routinely runs for hours. Whenever
//! Discord tells us something that could have removed someone's access, we
//! recompute the people currently in the affected rooms and evict the ones who
//! no longer belong.
//!
//! The sweep is deliberately blunt — recompute everyone in the room rather than
//! reason about which change affected whom. A room holds a handful of people,
//! the computation is in memory, and being clever here is how a revocation gets
//! missed.

use api::AppState;
use protocol::gateway::DispatchEvent;
use protocol::room::{RoomLeave, RoomLeaveReason};
use protocol::scalars::Snowflake;
use uuid::Uuid;

/// Recheck one room and evict whoever lost access.
pub async fn sweep_channel(state: &AppState, discord_channel_id: i64) {
    let participants =
        match db::repo::presence::list_by_channel(&state.pool, discord_channel_id).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, discord_channel_id, "listing room for revocation sweep");
                return;
            }
        };
    if participants.is_empty() {
        return;
    }

    let Ok(channel) = u64::try_from(discord_channel_id) else {
        return;
    };

    for participant in participants {
        let Ok(discord_user) = u64::try_from(participant.discord_user_id) else {
            continue;
        };
        let allowed = state
            .replica
            .permissions(discord_user, channel)
            .await
            .is_some_and(|p| p.can_join_room());
        if !allowed {
            evict(state, discord_channel_id, participant.user_id).await;
        }
    }
}

/// Recheck every room in a guild. Used when a role changed, since a role can
/// touch every channel at once.
pub async fn sweep_guild(state: &AppState, discord_guild_id: u64) {
    let channels = match db::repo::presence::occupied_channels(&state.pool).await {
        Ok(channels) => channels,
        Err(error) => {
            tracing::error!(%error, "listing occupied rooms for revocation sweep");
            return;
        }
    };
    for channel in channels {
        let Ok(id) = u64::try_from(channel) else {
            continue;
        };
        // So as salas do guild que mudou; varrer as outras seria trabalho a toa.
        if state
            .replica
            .channel_info(id)
            .await
            .is_some_and(|(guild, _)| guild == discord_guild_id)
        {
            sweep_channel(state, channel).await;
        }
    }
}

/// Remove one participant from the SFU and tell their client why.
async fn evict(state: &AppState, discord_channel_id: i64, user_id: Uuid) {
    tracing::info!(%user_id, discord_channel_id, "evicting: access revoked in discord");

    if let Err(error) = state
        .rooms
        .remove_participant(discord_channel_id, user_id)
        .await
    {
        // O LiveKit pode nao ter o participante (ja saiu). O estado local e
        // limpo de qualquer forma: manter a linha diria que ele ainda esta la.
        tracing::warn!(%error, %user_id, "livekit refused the removal");
    }
    if let Err(error) = db::repo::presence::leave(&state.pool, user_id).await {
        tracing::error!(%error, %user_id, "clearing presence after eviction");
    }

    state
        .hub
        .publish_to_user(
            user_id,
            DispatchEvent::RoomLeave(RoomLeave {
                discord_channel_id: Snowflake::new(discord_channel_id),
                reason: RoomLeaveReason::AccessRevoked,
            }),
        )
        .await;
}
