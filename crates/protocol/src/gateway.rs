//! WebSocket gateway wire format (`docs/protocol/websocket.md`).
//!
//! The gateway is a **notification** channel: the client only ever sends
//! `IDENTIFY`, `RESUME` and `HEARTBEAT`. Every mutation goes through REST.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::bridge::BridgeStatus;
use crate::channel::{Channel, DmParticipantEvent, ReadState};
use crate::guild::{Category, Member, Role};
use crate::message::{Message, MessageBulkDelete, MessageDelete, ReactionEvent, TypingStart};
use crate::user::{CurrentUser, Presence};
use crate::voice::VoiceState;

/// Frame opcode (`docs/protocol/websocket.md` §2.1). Serialised as an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(export, type = "0 | 1 | 2 | 3 | 4 | 5 | 6 | 7")]
pub struct Opcode(pub u8);

impl Opcode {
    /// S→C. Domain event; always carries `t` and `s`.
    pub const DISPATCH: Self = Self(0);
    /// S→C. First frame after connecting.
    pub const HELLO: Self = Self(1);
    /// C→S. Authentication with the access token.
    pub const IDENTIFY: Self = Self(2);
    /// C→S. Resume an existing session.
    pub const RESUME: Self = Self(3);
    /// C→S. Heartbeat.
    pub const HEARTBEAT: Self = Self(4);
    /// S→C. Heartbeat acknowledgement.
    pub const HEARTBEAT_ACK: Self = Self(5);
    /// S→C. Cannot resume; re-identify and recover the gap over REST.
    pub const INVALID_SESSION: Self = Self(6);
    /// S→C. The server asks for a reconnect (deployment); the session stays
    /// resumable.
    pub const RECONNECT: Self = Self(7);
}

/// Close codes (`docs/protocol/websocket.md` §3.5 and §8).
pub mod close_code {
    /// Unknown error. Client should resume.
    pub const UNKNOWN: u16 = 4000;
    /// Invalid authentication.
    pub const AUTHENTICATION_FAILED: u16 = 4001;
    /// Malformed frame.
    pub const DECODE_ERROR: u16 = 4002;
    /// Did not identify within 10 s.
    pub const NOT_AUTHENTICATED: u16 = 4003;
    /// A session with the same `session_id` is already active elsewhere.
    pub const SESSION_ALREADY_ACTIVE: u16 = 4004;
    /// Too many frames from the client.
    pub const RATE_LIMITED: u16 = 4008;
    /// Client protocol version below the supported minimum (§8).
    pub const VERSION_TOO_OLD: u16 = 4010;
    /// Client-initiated close after detecting a zombie connection.
    pub const ZOMBIED: u16 = 4900;
}

/// Limits from `docs/protocol/websocket.md` §7.
pub mod limits {
    /// Time allowed between `HELLO` and `IDENTIFY`.
    pub const IDENTIFY_TIMEOUT_MS: u64 = 10_000;
    /// Frames accepted from one client per window.
    pub const MAX_FRAMES_PER_WINDOW: u32 = 30;
    pub const FRAME_WINDOW_MS: u64 = 60_000;
    /// Largest frame accepted from a client.
    pub const MAX_FRAME_BYTES: usize = 4096;
    /// Simultaneous connections per user; the oldest is dropped beyond this.
    pub const MAX_CONNECTIONS_PER_USER: usize = 4;
}

// ---------------------------------------------------------------------------
// Frames do cliente
// ---------------------------------------------------------------------------

/// A frame received from the client, before `d` is interpreted.
///
/// `d` stays raw on purpose: the gateway dispatches on `op` first and only then
/// decides which payload type to parse, so a malformed `d` closes with 4002
/// instead of being silently coerced.
#[derive(Debug, Clone, Deserialize)]
pub struct RawClientFrame {
    pub op: Opcode,
    #[serde(default)]
    pub d: Option<serde_json::Value>,
}

/// `op: 2`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Identify {
    /// The same access token used by REST. The connection does **not** drop when
    /// it expires: authentication is checked at identification time only.
    pub token: String,
    pub client: ClientInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClientInfo {
    pub version: String,
    pub os: String,
}

/// `op: 3`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Resume {
    pub token: String,
    pub session_id: Uuid,
    #[ts(type = "number")]
    pub last_seq: u64,
}

// ---------------------------------------------------------------------------
// Frames do servidor
// ---------------------------------------------------------------------------

/// `op: 1`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Hello {
    #[ts(type = "number")]
    pub heartbeat_interval_ms: u64,
    #[ts(type = "number")]
    pub session_ttl_ms: u64,
}

/// `op: 6`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InvalidSession {
    pub resumable: bool,
}

/// A server frame that is not a dispatch: `HELLO`, `HEARTBEAT_ACK`,
/// `INVALID_SESSION` and `RECONNECT`.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ControlFrame<T> {
    pub op: Opcode,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub d: Option<T>,
}

impl ControlFrame<()> {
    pub fn heartbeat_ack() -> Self {
        Self {
            op: Opcode::HEARTBEAT_ACK,
            d: None,
        }
    }

    pub fn reconnect() -> Self {
        Self {
            op: Opcode::RECONNECT,
            d: None,
        }
    }
}

impl<T> ControlFrame<T> {
    pub fn new(op: Opcode, payload: T) -> Self {
        Self {
            op,
            d: Some(payload),
        }
    }
}

/// `op: 0`. `t` and `d` come from the flattened event; `s` is the per-session
/// monotonic sequence used only to detect a gap on resume (§6.1).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DispatchFrame {
    pub op: Opcode,
    #[ts(type = "number")]
    pub s: u64,
    #[serde(flatten)]
    #[ts(flatten)]
    pub event: DispatchEvent,
}

impl DispatchFrame {
    pub fn new(seq: u64, event: DispatchEvent) -> Self {
        Self {
            op: Opcode::DISPATCH,
            s: seq,
            event,
        }
    }
}

/// Every dispatch event (`docs/protocol/websocket.md` §5).
///
/// Adjacently tagged so it serialises exactly as `{"t": "NAME", "d": {…}}`, which
/// is what the envelope requires once flattened into `DispatchFrame`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "t", content = "d")]
pub enum DispatchEvent {
    #[serde(rename = "READY")]
    Ready(Box<Ready>),
    #[serde(rename = "RESUMED")]
    Resumed(Resumed),

    #[serde(rename = "MESSAGE_CREATE")]
    MessageCreate(Box<Message>),
    #[serde(rename = "MESSAGE_UPDATE")]
    MessageUpdate(Box<Message>),
    #[serde(rename = "MESSAGE_DELETE")]
    MessageDelete(MessageDelete),
    #[serde(rename = "MESSAGE_BULK_DELETE")]
    MessageBulkDelete(MessageBulkDelete),

    #[serde(rename = "REACTION_ADD")]
    ReactionAdd(ReactionEvent),
    #[serde(rename = "REACTION_REMOVE")]
    ReactionRemove(ReactionEvent),
    #[serde(rename = "TYPING_START")]
    TypingStart(TypingStart),

    #[serde(rename = "CHANNEL_CREATE")]
    ChannelCreate(Box<Channel>),
    #[serde(rename = "CHANNEL_UPDATE")]
    ChannelUpdate(Box<Channel>),
    #[serde(rename = "CHANNEL_DELETE")]
    ChannelDelete(Box<Channel>),

    #[serde(rename = "CATEGORY_CREATE")]
    CategoryCreate(Category),
    #[serde(rename = "CATEGORY_UPDATE")]
    CategoryUpdate(Category),
    #[serde(rename = "CATEGORY_DELETE")]
    CategoryDelete(Category),

    #[serde(rename = "ROLE_CREATE")]
    RoleCreate(Role),
    #[serde(rename = "ROLE_UPDATE")]
    RoleUpdate(Role),
    #[serde(rename = "ROLE_DELETE")]
    RoleDelete(Role),

    #[serde(rename = "GUILD_MEMBER_ADD")]
    GuildMemberAdd(Box<Member>),
    #[serde(rename = "GUILD_MEMBER_UPDATE")]
    GuildMemberUpdate(Box<Member>),
    #[serde(rename = "GUILD_MEMBER_REMOVE")]
    GuildMemberRemove(Box<Member>),

    /// Emitted after any role or overwrite change. Without it a demoted user
    /// keeps seeing controls the server will refuse.
    #[serde(rename = "PERMISSIONS_STALE")]
    PermissionsStale(PermissionsStale),

    #[serde(rename = "PRESENCE_UPDATE")]
    PresenceUpdate(Presence),
    #[serde(rename = "VOICE_STATE_UPDATE")]
    VoiceStateUpdate(VoiceState),

    #[serde(rename = "DM_CHANNEL_CREATE")]
    DmChannelCreate(Box<Channel>),
    #[serde(rename = "DM_PARTICIPANT_ADD")]
    DmParticipantAdd(DmParticipantEvent),
    #[serde(rename = "DM_PARTICIPANT_REMOVE")]
    DmParticipantRemove(DmParticipantEvent),

    /// Sent only to the user's own sessions.
    #[serde(rename = "READ_STATE_UPDATE")]
    ReadStateUpdate(ReadState),

    /// Sent only to administrators.
    #[serde(rename = "BRIDGE_STATUS")]
    BridgeStatus(BridgeStatus),
}

impl DispatchEvent {
    /// The `t` value, for logging and for the resume buffer filter.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Ready(_) => "READY",
            Self::Resumed(_) => "RESUMED",
            Self::MessageCreate(_) => "MESSAGE_CREATE",
            Self::MessageUpdate(_) => "MESSAGE_UPDATE",
            Self::MessageDelete(_) => "MESSAGE_DELETE",
            Self::MessageBulkDelete(_) => "MESSAGE_BULK_DELETE",
            Self::ReactionAdd(_) => "REACTION_ADD",
            Self::ReactionRemove(_) => "REACTION_REMOVE",
            Self::TypingStart(_) => "TYPING_START",
            Self::ChannelCreate(_) => "CHANNEL_CREATE",
            Self::ChannelUpdate(_) => "CHANNEL_UPDATE",
            Self::ChannelDelete(_) => "CHANNEL_DELETE",
            Self::CategoryCreate(_) => "CATEGORY_CREATE",
            Self::CategoryUpdate(_) => "CATEGORY_UPDATE",
            Self::CategoryDelete(_) => "CATEGORY_DELETE",
            Self::RoleCreate(_) => "ROLE_CREATE",
            Self::RoleUpdate(_) => "ROLE_UPDATE",
            Self::RoleDelete(_) => "ROLE_DELETE",
            Self::GuildMemberAdd(_) => "GUILD_MEMBER_ADD",
            Self::GuildMemberUpdate(_) => "GUILD_MEMBER_UPDATE",
            Self::GuildMemberRemove(_) => "GUILD_MEMBER_REMOVE",
            Self::PermissionsStale(_) => "PERMISSIONS_STALE",
            Self::PresenceUpdate(_) => "PRESENCE_UPDATE",
            Self::VoiceStateUpdate(_) => "VOICE_STATE_UPDATE",
            Self::DmChannelCreate(_) => "DM_CHANNEL_CREATE",
            Self::DmParticipantAdd(_) => "DM_PARTICIPANT_ADD",
            Self::DmParticipantRemove(_) => "DM_PARTICIPANT_REMOVE",
            Self::ReadStateUpdate(_) => "READ_STATE_UPDATE",
            Self::BridgeStatus(_) => "BRIDGE_STATUS",
        }
    }

    /// `TYPING_START` is ephemeral and never enters the resume buffer: replaying
    /// a 40-second-old typing indicator is noise (§5).
    pub const fn is_replayable(&self) -> bool {
        !matches!(self, Self::TypingStart(_))
    }
}

/// `READY` payload (§3.1). Carries **structure**, never history: a `READY` that
/// loaded messages would make startup O(n) in server size.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Ready {
    pub session_id: Uuid,
    pub user: CurrentUser,
    /// Only guilds and channels the user can `VIEW_CHANNEL` at identification.
    pub guilds: Vec<ReadyGuild>,
    pub dm_channels: Vec<Channel>,
    pub read_states: Vec<ReadState>,
    pub presences: Vec<Presence>,
    pub voice_states: Vec<VoiceState>,
    /// Heartbeat interval echoed for clients that reconnect without a new HELLO.
    #[ts(type = "number")]
    pub heartbeat_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReadyGuild {
    pub id: Uuid,
    pub name: String,
    pub icon_url: Option<String>,
    pub owner_id: Uuid,
    pub categories: Vec<Category>,
    pub channels: Vec<Channel>,
    pub roles: Vec<Role>,
    pub members: Vec<Member>,
    #[ts(type = "number")]
    pub member_count: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Resumed {
    pub replayed: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PermissionsStale {
    pub guild_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_frame_matches_the_documented_envelope() {
        let frame = DispatchFrame::new(
            4211,
            DispatchEvent::PermissionsStale(PermissionsStale {
                guild_id: Uuid::nil(),
            }),
        );
        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["op"], 0);
        assert_eq!(json["t"], "PERMISSIONS_STALE");
        assert_eq!(json["s"], 4211);
        assert_eq!(json["d"]["guild_id"], Uuid::nil().to_string());
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["d", "op", "s", "t"]);
    }

    #[test]
    fn control_frames_omit_d_when_there_is_no_payload() {
        let ack = serde_json::to_value(ControlFrame::heartbeat_ack()).unwrap();
        assert_eq!(ack, serde_json::json!({ "op": 5 }));
        let reconnect = serde_json::to_value(ControlFrame::reconnect()).unwrap();
        assert_eq!(reconnect, serde_json::json!({ "op": 7 }));
    }

    #[test]
    fn hello_carries_both_intervals() {
        let hello = ControlFrame::new(
            Opcode::HELLO,
            Hello {
                heartbeat_interval_ms: 30_000,
                session_ttl_ms: 90_000,
            },
        );
        let json = serde_json::to_value(&hello).unwrap();
        assert_eq!(json["op"], 1);
        assert_eq!(json["d"]["heartbeat_interval_ms"], 30_000);
        assert_eq!(json["d"]["session_ttl_ms"], 90_000);
    }

    #[test]
    fn raw_client_frame_accepts_a_heartbeat_without_d() {
        let frame: RawClientFrame = serde_json::from_str(r#"{"op":4}"#).unwrap();
        assert_eq!(frame.op, Opcode::HEARTBEAT);
        assert!(frame.d.is_none());
    }

    #[test]
    fn typing_start_is_the_only_event_excluded_from_the_resume_buffer() {
        let typing = DispatchEvent::TypingStart(TypingStart {
            channel_id: Uuid::nil(),
            user_id: Uuid::nil(),
            expires_at: crate::Timestamp::new(time::OffsetDateTime::UNIX_EPOCH),
        });
        assert!(!typing.is_replayable());
        assert!(DispatchEvent::Resumed(Resumed { replayed: 1 }).is_replayable());
    }

    #[test]
    fn every_event_name_is_screaming_snake_case() {
        for event in [
            DispatchEvent::Resumed(Resumed { replayed: 0 }),
            DispatchEvent::PermissionsStale(PermissionsStale {
                guild_id: Uuid::nil(),
            }),
        ] {
            let name = event.name();
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                "{name} não está em SCREAMING_SNAKE_CASE"
            );
            let json = serde_json::to_value(&event).unwrap();
            assert_eq!(json["t"], name, "name() diverge do serde rename");
        }
    }
}
