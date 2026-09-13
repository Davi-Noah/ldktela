//! The `READY` payload (`docs/websocket.md` §3.1).
//!
//! Small by design. v1's READY carried the whole guild structure because the
//! client had a tree to draw; this one carries the user and, when there is one,
//! the room they are already in. Most of the time that is `None`, because most
//! of the time the app is sitting in the tray with nothing to show.

use protocol::gateway::Ready;
use uuid::Uuid;

use crate::state::AppState;

pub async fn build(state: &AppState, user_id: Uuid, session_id: Uuid) -> Ready {
    let user = match db::repo::users::find_by_id(&state.pool, user_id).await {
        Ok(row) => row.to_current(),
        Err(error) => {
            // Chegar aqui significa que o token e valido para um usuario que
            // sumiu do banco. Nao ha READY honesto a montar.
            tracing::error!(%error, %user_id, "building READY for a missing user");
            return Ready {
                session_id,
                user: placeholder_user(user_id),
                room: None,
                heartbeat_interval_ms: state.hub.config().heartbeat_interval_ms,
            };
        }
    };

    Ready {
        session_id,
        user,
        room: current_room(state, user_id).await,
        heartbeat_interval_ms: state.hub.config().heartbeat_interval_ms,
    }
}

async fn current_room(state: &AppState, user_id: Uuid) -> Option<protocol::room::RoomState> {
    let channel = db::repo::presence::channel_of(&state.pool, user_id)
        .await
        .ok()
        .flatten()?;
    let (guild, name) = state
        .replica
        .channel_info(u64::try_from(channel).ok()?)
        .await?;
    crate::routes::rooms::state_of(
        state,
        channel,
        i64::try_from(guild).unwrap_or_default(),
        name,
    )
    .await
    .ok()
}

/// Only reachable from the error branch above, where the alternative is to drop
/// the connection with no explanation.
fn placeholder_user(id: Uuid) -> protocol::user::CurrentUser {
    protocol::user::CurrentUser {
        id,
        discord_user_id: protocol::scalars::Snowflake::new(0),
        username: String::new(),
        display_name: None,
        avatar_url: None,
        created_at: protocol::scalars::Timestamp::new(time::OffsetDateTime::UNIX_EPOCH),
    }
}
