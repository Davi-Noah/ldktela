//! WebSocket gateway wire format (`docs/websocket.md`).
//!
//! The gateway is a **notification** channel: the client only ever sends
//! `IDENTIFY`, `RESUME` and `HEARTBEAT`. Every mutation goes through REST.
//!
//! The frame machinery below is unchanged from v1 — it is the most mature part
//! of the project. What shrank is the event set: from thirty events describing a
//! chat application to eight describing a room.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::room::{
    RoomLeave, RoomParticipantAdd, RoomParticipantRemove, RoomState, ShareStart, ShareStop,
};
use crate::user::CurrentUser;

/// Frame opcode (`docs/websocket.md` §2.1). Serialised as an integer.
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

/// Close codes (`docs/websocket.md` §3.5 and §8).
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

/// Limits from `docs/websocket.md` §7.
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

/// Every dispatch event (`docs/websocket.md` §5).
///
/// Adjacently tagged so it serialises exactly as `{"t": "NAME", "d": {…}}`, which
/// is what the envelope requires once flattened into `DispatchFrame`.
///
/// Every event here is replayable. v1 had one ephemeral event (`TYPING_START`)
/// that consumed a sequence without entering the resume buffer; with it gone,
/// the buffer holds everything the session ever sent.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "t", content = "d")]
pub enum DispatchEvent {
    #[serde(rename = "READY")]
    Ready(Box<Ready>),
    #[serde(rename = "RESUMED")]
    Resumed(Resumed),

    /// The user entered a Discord voice channel and may now open the room.
    #[serde(rename = "ROOM_JOIN")]
    RoomJoin(Box<RoomState>),
    /// The user left, or was removed because access disappeared (RF-08).
    #[serde(rename = "ROOM_LEAVE")]
    RoomLeave(RoomLeave),

    #[serde(rename = "ROOM_PARTICIPANT_ADD")]
    RoomParticipantAdd(Box<RoomParticipantAdd>),
    #[serde(rename = "ROOM_PARTICIPANT_REMOVE")]
    RoomParticipantRemove(RoomParticipantRemove),

    #[serde(rename = "SHARE_START")]
    ShareStart(ShareStart),
    #[serde(rename = "SHARE_STOP")]
    ShareStop(ShareStop),
}

impl DispatchEvent {
    /// The `t` value, for logging and for tests that assert the envelope.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Ready(_) => "READY",
            Self::Resumed(_) => "RESUMED",
            Self::RoomJoin(_) => "ROOM_JOIN",
            Self::RoomLeave(_) => "ROOM_LEAVE",
            Self::RoomParticipantAdd(_) => "ROOM_PARTICIPANT_ADD",
            Self::RoomParticipantRemove(_) => "ROOM_PARTICIPANT_REMOVE",
            Self::ShareStart(_) => "SHARE_START",
            Self::ShareStop(_) => "SHARE_STOP",
        }
    }
}

/// `READY` payload (§3.1).
///
/// `room` is `None` whenever the user is not in a Discord voice channel, which
/// is most of the time: the app sits in the tray and only has something to show
/// when Discord says so (ADR-0011).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Ready {
    pub session_id: Uuid,
    pub user: CurrentUser,
    // `skip_serializing_if` e obrigatorio junto de `ts(optional)`: sem ele o
    // serde emite `"room": null`, o tipo gerado promete um campo ausente, e o
    // cliente que testa `=== undefined` recebe `null` e quebra.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub room: Option<RoomState>,
    /// Heartbeat interval echoed for clients that reconnect without a new HELLO.
    #[ts(type = "number")]
    pub heartbeat_interval_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Resumed {
    pub replayed: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::{PublicationSource, RoomLeaveReason};
    use crate::scalars::{Snowflake, Timestamp};

    #[test]
    fn dispatch_frame_matches_the_documented_envelope() {
        let frame = DispatchFrame::new(
            4211,
            DispatchEvent::ShareStart(ShareStart {
                discord_channel_id: Snowflake::new(42),
                user_id: Uuid::nil(),
                source: PublicationSource::Screen,
                started_at: Timestamp::new(time::OffsetDateTime::UNIX_EPOCH),
            }),
        );
        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["op"], 0);
        assert_eq!(json["t"], "SHARE_START");
        assert_eq!(json["d"]["source"], "screen");
        assert_eq!(json["s"], 4211);
        assert_eq!(json["d"]["user_id"], Uuid::nil().to_string());
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["d", "op", "s", "t"]);
    }

    #[test]
    fn snowflakes_in_events_stay_strings_on_the_wire() {
        // 2^53 + 1 perde precisao em Number; um id de canal real passa disso.
        let frame = DispatchFrame::new(
            1,
            DispatchEvent::ShareStop(ShareStop {
                discord_channel_id: Snowflake::new(9_007_199_254_740_993),
                user_id: Uuid::nil(),
                source: PublicationSource::Camera,
            }),
        );
        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["d"]["discord_channel_id"], "9007199254740993");
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
    fn room_leave_reason_serialises_in_snake_case() {
        let frame = DispatchFrame::new(
            7,
            DispatchEvent::RoomLeave(RoomLeave {
                discord_channel_id: Snowflake::new(1),
                reason: RoomLeaveReason::AccessRevoked,
            }),
        );
        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["d"]["reason"], "access_revoked");
    }

    #[test]
    fn every_event_name_is_screaming_snake_case() {
        for event in [
            DispatchEvent::Resumed(Resumed { replayed: 0 }),
            DispatchEvent::ShareStart(ShareStart {
                discord_channel_id: Snowflake::new(1),
                user_id: Uuid::nil(),
                source: PublicationSource::Screen,
                started_at: Timestamp::new(time::OffsetDateTime::UNIX_EPOCH),
            }),
            DispatchEvent::RoomLeave(RoomLeave {
                discord_channel_id: Snowflake::new(1),
                reason: RoomLeaveReason::Left,
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
