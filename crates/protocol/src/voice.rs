//! Voice (`docs/api/rest-api.md` §6.9, RF-19 to RF-23, RNF-07, RNF-10).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// `POST /channels/{id}/voice-token`. The backend checks `CONNECT_VOICE` and the
/// RNF-10 guards before issuing.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct VoiceTokenRequest {
    /// Whether the client intends to publish a camera. Drives the three-publisher
    /// guard: a listener is never refused because the cameras are full.
    #[serde(default)]
    pub publish_camera: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VoiceTokenResponse {
    pub token: String,
    /// WebSocket URL of the SFU.
    pub url: String,
    /// LiveKit room name for this channel.
    pub room: String,
    /// Token lifetime in seconds, always ≤ 3600 (RNF-07).
    #[ts(type = "number")]
    pub expires_in: i64,
}

/// One row of `voice_states`, and the body of `VOICE_STATE_UPDATE`.
///
/// `channel_id: null` means the user left. This event is fed exclusively by the
/// LiveKit webhooks received by the backend, never by the client (RF-20).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VoiceState {
    pub user_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub streaming: bool,
}

/// `PATCH /voice-states/@me`. The client is the source of truth for these two
/// flags and nothing else.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct UpdateVoiceStateRequest {
    #[ts(optional)]
    pub self_mute: Option<bool>,
    #[ts(optional)]
    pub self_deaf: Option<bool>,
}
