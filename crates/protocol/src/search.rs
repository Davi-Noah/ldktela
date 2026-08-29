//! Full-text search (`docs/api/rest-api.md` §6.8, RF-17).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::message::Message;
use crate::scalars::Timestamp;

/// `GET /search`. Scope is `guild_id` **or** `channel_id`; there is no global
/// search without a scope.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct SearchQuery {
    pub q: String,
    #[ts(optional)]
    pub guild_id: Option<Uuid>,
    #[ts(optional)]
    pub channel_id: Option<Uuid>,
    #[ts(optional)]
    pub author_id: Option<Uuid>,
    /// Inclusive lower bound on `created_at`.
    #[ts(optional)]
    pub since: Option<Timestamp>,
    /// Exclusive upper bound on `created_at`.
    #[ts(optional)]
    pub until: Option<Timestamp>,
    /// Keyset cursor: results strictly older than this message id.
    #[ts(optional)]
    pub before: Option<Uuid>,
    #[ts(optional)]
    pub limit: Option<u32>,
}

/// One hit. Carries the neighbouring ids so the client can open the channel with
/// `around` without a second round trip.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchHit {
    pub message: Message,
    pub previous_message_id: Option<Uuid>,
    pub next_message_id: Option<Uuid>,
}
