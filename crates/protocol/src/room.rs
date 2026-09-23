//! Rooms (RF-10 to RF-17).
//!
//! A room *is* a Discord voice channel (ADR-0011): it is addressed by the
//! channel's snowflake, and the client never picks one — it is told which room
//! it is in, because Discord already knows.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::scalars::{Snowflake, Timestamp};
use crate::user::UserSummary;

/// `POST /rooms/{discord_channel_id}/token`.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct RoomTokenRequest {
    /// Everything the client intends to have live, not just what it is starting
    /// now (ADR-0038).
    ///
    /// The list is read as the whole intent: sources named here are claimed,
    /// sources left out are given back. Empty is a viewer, and a viewer is never
    /// refused because the publisher slots are full.
    ///
    /// Declaring the full set is what makes the call idempotent — the client
    /// renews its token every hour (RNF-06), and a renewal must not be read as
    /// a second publisher nor drop the slot of a source already on the air.
    #[serde(default)]
    pub publish: Vec<PublicationSource>,
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

/// What a publication carries: a screen, or a face (ADR-0038).
///
/// The pair (person, source) is the unit of the domain. A person may hold one
/// of each at the same time, and each is started, stopped, watched and left on
/// its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum PublicationSource {
    Screen,
    Camera,
}

/// One live publication of one person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Publication {
    pub source: PublicationSource,
    /// When this publication went live, from the server (RF-34).
    ///
    /// Not the moment the viewer joined: someone who arrives twenty minutes in
    /// has to see twenty minutes, not zero.
    pub since: Timestamp,
}

/// Someone present in a room.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomParticipant {
    pub user: UserSummary,
    /// What this person is transmitting right now, empty when nothing.
    ///
    /// A list, and not a pair of booleans, because the viewer builds one tile
    /// per entry: adding a source must not add a field to everything that
    /// renders a participant (ADR-0038).
    pub publications: Vec<Publication>,
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

/// Body of `SHARE_START`. Fed exclusively by LiveKit webhooks, never by the
/// client.
///
/// Separate from `ShareStop` because it carries `started_at`, and a stop event
/// with a start time would be a field that is always a lie.
///
/// `source` says which publication started (ADR-0038): the same person emits one
/// of these per source, and a client that only knew about screens would silently
/// treat a camera as one.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShareStart {
    pub discord_channel_id: Snowflake,
    pub user_id: Uuid,
    pub source: PublicationSource,
    pub started_at: Timestamp,
}

/// Body of `SHARE_STOP`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShareStop {
    pub discord_channel_id: Snowflake,
    pub user_id: Uuid,
    pub source: PublicationSource,
}
