//! Messages, attachments and reactions (`docs/api/rest-api.md` §6.5, §6.6, §7).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::scalars::Timestamp;
use crate::user::UserSummary;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Message {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub author: UserSummary,
    pub content: String,
    pub reply_to: Option<ReplyPreview>,
    pub attachments: Vec<Attachment>,
    pub reactions: Vec<Reaction>,
    pub is_pinned: bool,
    pub edited_at: Option<Timestamp>,
    pub created_at: Timestamp,
    /// Echoed back only to the session that sent the message, so the optimistic
    /// insert can be reconciled. `null` for every other session.
    pub nonce: Option<String>,
    pub bridge: Option<BridgeInfo>,
}

/// Enough of the referenced message to render the reply header (RF-14).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReplyPreview {
    pub id: Uuid,
    pub author_username: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Attachment {
    pub id: Uuid,
    pub filename: String,
    pub content_type: String,
    #[ts(type = "number")]
    pub size_bytes: i64,
    /// Persisted so the list reserves space and does not reflow while scrolling
    /// (RNF-04).
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// `null` when the attachment was not migrated (RF-25a).
    pub url: Option<String>,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Reaction {
    pub emoji: String,
    #[ts(type = "number")]
    pub count: i64,
    /// Whether the requester reacted.
    pub me: bool,
}

/// Present when the message crossed the Discord bridge (RF-29).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BridgeInfo {
    pub origin: MessageOrigin,
}

/// Mirrors the `message_origin` enum in the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum MessageOrigin {
    Internal,
    Discord,
}

/// `POST /channels/{id}/messages`.
///
/// Mentions are **not** in this body: the server extracts them from `content`.
/// A client-supplied list is trustworthy only until someone opens DevTools.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateMessageRequest {
    #[serde(default)]
    pub content: String,
    /// Client-generated. Idempotent for 60 s within the same channel: repeating
    /// it returns the existing message with `200` instead of `201`.
    #[ts(optional)]
    pub nonce: Option<String>,
    #[ts(optional)]
    pub reply_to_id: Option<Uuid>,
    #[serde(default)]
    pub attachments: Vec<AttachmentInput>,
}

/// One already-uploaded object being attached to a message. The backend confirms
/// it exists in R2 with a `HEAD` before persisting (SRS §6.2).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct AttachmentInput {
    pub r2_key: String,
    pub filename: String,
    pub content_type: String,
    #[ts(type = "number")]
    pub size_bytes: i64,
    #[ts(optional)]
    pub width: Option<i32>,
    #[ts(optional)]
    pub height: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct EditMessageRequest {
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MessageDelete {
    pub id: Uuid,
    pub channel_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MessageBulkDelete {
    pub ids: Vec<Uuid>,
    pub channel_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReactionEvent {
    pub message_id: Uuid,
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub emoji: String,
}

/// `TYPING_START`. Ephemeral, never persisted, never replayed on resume (RF-13).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TypingStart {
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub expires_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Anexos (§6.6, RF-10, RF-11a)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct PresignRequest {
    pub channel_id: Uuid,
    pub filename: String,
    pub content_type: String,
    #[ts(type = "number")]
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PresignResponse {
    pub r2_key: String,
    pub upload_url: String,
    #[ts(type = "number")]
    pub expires_in: i64,
}
