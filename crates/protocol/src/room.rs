//! Rooms (RF-10 to RF-17).
//!
//! A room *is* a Discord voice channel (ADR-0011): it is addressed by the
//! channel's snowflake, and the client never picks one — it is told which room
//! it is in, because Discord already knows.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::scalars::Snowflake;
use crate::user::UserSummary;

/// `POST /rooms/{discord_channel_id}/token`.
#[derive(Debug, Clone, Copy, Default, Deserialize, TS)]
#[ts(export)]
pub struct RoomTokenRequest {
    /// Whether the client intends to publish a screen. Drives the admission
    /// guard: a viewer is never refused because the publisher slots are full.
    #[serde(default)]
    pub publish: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomTokenResponse {
    pub token: String,
    /// WebSocket URL of the SFU.
    pub url: String,
    /// LiveKit room name for this Discord voice channel.
    pub room: String,
    /// Token lifetime in seconds, always <= 3600 (RNF-06).
    #[ts(type = "number")]
    pub expires_in: i64,
}

/// Someone present in a room. Presence here means "connected to the Discord
/// voice channel", which is the only definition the product has.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomParticipant {
    pub user: UserSummary,
    pub publishing: bool,
}

/// The full state of one room, carried by `ROOM_JOIN`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomState {
    pub discord_channel_id: Snowflake,
    pub discord_guild_id: Snowflake,
    /// Human-readable name of the Discord voice channel, for the window title.
    pub channel_name: String,
    pub participants: Vec<RoomParticipant>,
}

/// Why the server took the client out of a room. `Left` is the ordinary case;
/// the other two exist so the UI can say something true instead of "disconnected".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum RoomLeaveReason {
    /// The user left the Discord voice channel.
    Left,
    /// Access was revoked in Discord while the session was live (RF-08).
    AccessRevoked,
    /// The replica went stale, so the server can no longer vouch for access.
    ReplicaStale,
}

/// Body of `ROOM_LEAVE`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomLeave {
    pub discord_channel_id: Snowflake,
    pub reason: RoomLeaveReason,
}

/// Body of `ROOM_PARTICIPANT_ADD`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomParticipantAdd {
    pub discord_channel_id: Snowflake,
    pub participant: RoomParticipant,
}

/// Body of `ROOM_PARTICIPANT_REMOVE`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomParticipantRemove {
    pub discord_channel_id: Snowflake,
    pub user_id: Uuid,
}

/// Body of `SHARE_START` and `SHARE_STOP`. Fed exclusively by LiveKit webhooks,
/// never by the client.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShareEvent {
    pub discord_channel_id: Snowflake,
    pub user_id: Uuid,
}
