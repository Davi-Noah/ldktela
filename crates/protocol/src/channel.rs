//! Channels, including direct messages (`docs/api/rest-api.md` §7, §6.3, §6.7).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::patch::double_option;
use crate::scalars::{PermissionMask, Snowflake, Timestamp};
use crate::user::UserSummary;

/// Mirrors the `channel_type` enum in the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Text,
    Voice,
    Dm,
    GroupDm,
}

impl ChannelType {
    /// Direct conversations short-circuit permission resolution at step 0 and
    /// can never be bridged (RF-18a).
    pub const fn is_direct(self) -> bool {
        matches!(self, Self::Dm | Self::GroupDm)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Voice => "voice",
            Self::Dm => "dm",
            Self::GroupDm => "group_dm",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Channel {
    pub id: Uuid,
    pub guild_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub name: String,
    pub topic: Option<String>,
    #[serde(rename = "type")]
    pub kind: ChannelType,
    pub position: i32,
    pub bridge_enabled: bool,
    pub discord_channel_id: Option<Snowflake>,
    /// Only present on `dm` and `group_dm`.
    pub participants: Option<Vec<UserSummary>>,
    /// The requester's already-resolved mask (SRS §5.3). The client uses it to
    /// enable or hide controls and never re-implements resolution.
    pub permissions: PermissionMask,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateChannelRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ChannelType,
    #[ts(optional)]
    pub category_id: Option<Uuid>,
    #[ts(optional)]
    pub topic: Option<String>,
    #[ts(optional)]
    pub position: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct UpdateChannelRequest {
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub topic: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub category_id: Option<Option<Uuid>>,
    #[ts(optional)]
    pub position: Option<i32>,
}

/// `PATCH /guilds/{id}/channels/positions` — batch reorder, transactional.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct ChannelPosition {
    pub id: Uuid,
    pub position: i32,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub category_id: Option<Option<Uuid>>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct ReorderChannelsRequest {
    pub positions: Vec<ChannelPosition>,
}

// ---------------------------------------------------------------------------
// Conversas diretas (§6.7, RF-18, RF-18b)
// ---------------------------------------------------------------------------

/// `POST /dms`. With a single recipient the server resolves the existing 1:1
/// channel instead of creating another (RF-18).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateDmRequest {
    pub recipient_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct AddDmParticipantRequest {
    pub user_id: Uuid,
}

/// Body of `DM_PARTICIPANT_ADD` and `DM_PARTICIPANT_REMOVE`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DmParticipantEvent {
    pub channel_id: Uuid,
    pub user_id: Uuid,
}

// ---------------------------------------------------------------------------
// Estado de leitura (RF-16, §6.5)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReadState {
    pub channel_id: Uuid,
    pub last_read_message_id: Option<Uuid>,
    /// Authoritative on the server; the client displays it and never recomputes
    /// it (`docs/protocol/websocket.md` §6.5).
    pub mention_count: i32,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct UpdateReadStateRequest {
    pub last_read_message_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_matches_the_schema_enum_labels() {
        assert_eq!(
            serde_json::to_string(&ChannelType::GroupDm).unwrap(),
            "\"group_dm\""
        );
        assert_eq!(ChannelType::Text.as_str(), "text");
        assert!(ChannelType::Dm.is_direct());
        assert!(ChannelType::GroupDm.is_direct());
        assert!(!ChannelType::Voice.is_direct());
    }

    #[test]
    fn channel_serialises_type_under_the_key_type() {
        let json = serde_json::to_value(Channel {
            id: Uuid::nil(),
            guild_id: None,
            category_id: None,
            name: "geral".into(),
            topic: None,
            kind: ChannelType::Text,
            position: 0,
            bridge_enabled: false,
            discord_channel_id: None,
            participants: None,
            permissions: PermissionMask::new(384),
            created_at: Timestamp::new(time::OffsetDateTime::UNIX_EPOCH),
        })
        .unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["permissions"], "384");
    }
}
