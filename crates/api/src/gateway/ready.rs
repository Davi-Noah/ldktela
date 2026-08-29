//! The `READY` payload (`docs/protocol/websocket.md` §3.1).
//!
//! `READY` carries **structure**, never history: channels, categories, roles,
//! members, read state, presence and voice state. Messages come per channel over
//! REST, on demand. A `READY` that loaded history would make startup O(n) in the
//! size of the server.
//!
//! It also carries only the channels the user can `VIEW_CHANNEL` **at the moment
//! of identification** — resolved per channel, not read from the routing index.

use db::repo::{categories, channels, guilds, permissions, roles, users, voice_states};
use domain::Permissions;
use protocol::channel::{Channel, ReadState};
use protocol::gateway::{Ready, ReadyGuild};
use protocol::user::PresenceStatus;
use uuid::Uuid;

use crate::state::AppState;

pub async fn build(state: &AppState, user_id: Uuid, session_id: Uuid) -> Ready {
    let user = match users::find_by_id(&state.pool, user_id).await {
        Ok(row) => row,
        Err(err) => {
            // Only reachable if the account vanished between token issue and
            // identification. Nothing useful can be said, so send an empty
            // structure and let the client's next REST call fail loudly.
            tracing::error!(%user_id, error = %err, "READY for an unknown user");
            return empty_ready(session_id, user_id, state);
        }
    };

    let mut ready_guilds = Vec::new();
    for guild in guilds::list_for_user(&state.pool, user_id)
        .await
        .unwrap_or_default()
    {
        let mut visible = Vec::new();
        for channel in channels::list_by_guild(&state.pool, guild.id)
            .await
            .unwrap_or_default()
        {
            let Ok(Some(mask)) =
                permissions::resolve_for_channel(&state.pool, user_id, channel.id).await
            else {
                continue;
            };
            if mask.contains(Permissions::VIEW_CHANNEL) {
                visible.push(channel.to_wire(mask.bits(), None));
            }
        }
        let members = guilds::list_members(&state.pool, guild.id)
            .await
            .unwrap_or_default();
        ready_guilds.push(ReadyGuild {
            id: guild.id,
            name: guild.name.clone(),
            icon_url: guild.icon_url.clone(),
            owner_id: guild.owner_id,
            categories: categories::list_by_guild(&state.pool, guild.id)
                .await
                .unwrap_or_default()
                .iter()
                .map(|c| c.to_wire())
                .collect(),
            channels: visible,
            roles: roles::list_by_guild(&state.pool, guild.id)
                .await
                .unwrap_or_default()
                .iter()
                .map(|r| r.to_wire())
                .collect(),
            member_count: members.len() as i64,
            members: members.iter().map(|m| m.to_wire()).collect(),
        });
    }

    let mut dm_channels: Vec<Channel> = Vec::new();
    for channel in channels::list_direct_for_user(&state.pool, user_id)
        .await
        .unwrap_or_default()
    {
        let participants = channels::participants(&state.pool, channel.id)
            .await
            .unwrap_or_default();
        dm_channels.push(channel.to_wire(Permissions::DIRECT_MESSAGE.bits(), Some(participants)));
    }

    // Voice state is visible to everyone in the guild, including people who
    // are not connected to the room (RF-20).
    let mut voice = Vec::new();
    for guild in &ready_guilds {
        voice.extend(
            voice_states::list_by_guild(&state.pool, guild.id)
                .await
                .unwrap_or_default(),
        );
    }

    Ready {
        session_id,
        user: user.to_current(state.hub.presence_of(user_id, user_id).await),
        guilds: ready_guilds,
        dm_channels,
        read_states: read_states(state, user_id).await,
        presences: state.hub.presences_for(&state.pool, user_id).await,
        voice_states: voice,
        heartbeat_interval_ms: state.hub.config().heartbeat_interval_ms,
    }
}

/// Read state is per user and never filtered by channel visibility: a row for a
/// channel the user can no longer see is harmless, and dropping it would reset
/// their unread marker if access came back.
async fn read_states(state: &AppState, user_id: Uuid) -> Vec<ReadState> {
    sqlx::query!(
        "SELECT channel_id, last_read_message_id, mention_count, muted \
         FROM read_states WHERE user_id = $1",
        user_id
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|r| ReadState {
        channel_id: r.channel_id,
        last_read_message_id: r.last_read_message_id,
        mention_count: r.mention_count,
        muted: r.muted,
    })
    .collect()
}

fn empty_ready(session_id: Uuid, user_id: Uuid, state: &AppState) -> Ready {
    Ready {
        session_id,
        user: protocol::user::CurrentUser {
            id: user_id,
            email: None,
            username: String::new(),
            display_name: None,
            avatar_url: None,
            accent_color: None,
            bio: None,
            discord_user_id: None,
            status: PresenceStatus::Offline,
            created_at: protocol::Timestamp::new(time::OffsetDateTime::UNIX_EPOCH),
        },
        guilds: Vec::new(),
        dm_channels: Vec::new(),
        read_states: Vec::new(),
        presences: Vec::new(),
        voice_states: Vec::new(),
        heartbeat_interval_ms: state.hub.config().heartbeat_interval_ms,
    }
}
