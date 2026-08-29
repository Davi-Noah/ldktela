//! Users, profiles and presence (`docs/api/rest-api.md` §6.1, RF-03, RF-04).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::patch::double_option;
use crate::scalars::{Snowflake, Timestamp};

/// The author/participant shape embedded in messages and channels
/// (`docs/api/rest-api.md` §7).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UserSummary {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub accent_color: Option<String>,
    /// Ghost user imported from the Discord history (RF-26).
    pub is_migrated: bool,
}

/// Public profile (`GET /users/{id}`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub accent_color: Option<String>,
    pub bio: Option<String>,
    pub is_migrated: bool,
    pub created_at: Timestamp,
}

/// `GET /users/@me`. Carries the fields no one else may see.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CurrentUser {
    pub id: Uuid,
    pub email: Option<String>,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub accent_color: Option<String>,
    pub bio: Option<String>,
    pub discord_user_id: Option<Snowflake>,
    /// The user's own status, which is the only place `invisible` is reported.
    pub status: PresenceStatus,
    pub created_at: Timestamp,
}

/// `PATCH /users/@me`. Absent field = keep, `null` = clear (§1).
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateCurrentUserRequest {
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub display_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub avatar_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub bio: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub accent_color: Option<Option<String>>,
}

/// RF-04. `invisible` is reported to third parties as `offline`; only the user
/// themself ever sees it (`docs/protocol/websocket.md` §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum PresenceStatus {
    Online,
    Idle,
    Dnd,
    Invisible,
    Offline,
}

impl PresenceStatus {
    /// What third parties are allowed to see.
    pub const fn visible_to_others(self) -> Self {
        match self {
            Self::Invisible => Self::Offline,
            other => other,
        }
    }

    /// `online` and `offline` derive from the heartbeat, not from the client
    /// (`docs/api/rest-api.md` §6.1), so they are not settable.
    pub const fn is_settable(self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Dnd | Self::Invisible | Self::Online
        )
    }
}

/// `PATCH /users/@me/presence`.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct UpdatePresenceRequest {
    pub status: PresenceStatus,
}

/// One entry of `READY.presences` and the body of `PRESENCE_UPDATE`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Presence {
    pub user_id: Uuid,
    pub status: PresenceStatus,
}

/// `POST /users/@me/discord-link` (RF-26a). Transactional on the server.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct DiscordLinkRequest {
    pub discord_user_id: Snowflake,
}

/// Result of linking: how many ghost messages were reattributed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiscordLinkResponse {
    #[ts(type = "number")]
    pub reattributed_messages: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_is_never_shown_to_third_parties() {
        assert_eq!(
            PresenceStatus::Invisible.visible_to_others(),
            PresenceStatus::Offline
        );
        assert_eq!(PresenceStatus::Dnd.visible_to_others(), PresenceStatus::Dnd);
    }

    #[test]
    fn presence_status_serialises_in_lowercase() {
        assert_eq!(
            serde_json::to_string(&PresenceStatus::Dnd).unwrap(),
            "\"dnd\""
        );
        assert_eq!(
            serde_json::from_str::<PresenceStatus>("\"idle\"").unwrap(),
            PresenceStatus::Idle
        );
    }
}
