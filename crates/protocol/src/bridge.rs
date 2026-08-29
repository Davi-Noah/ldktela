//! Discord bridge administration (`docs/api/rest-api.md` §6.10, RF-31, RF-31a).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Body of the `BRIDGE_STATUS` dispatch and of `GET /admin/bridge/status`.
/// Sent only to administrators. A permanent bridge that dies silently is worse
/// than no bridge at all.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BridgeStatus {
    pub connected: bool,
    /// Undelivered rows in `bridge_outbox`.
    #[ts(type = "number")]
    pub queue_depth: i64,
    pub last_error: Option<String>,
}

/// `POST /admin/bridge/channels/{id}` — enables the bridge and registers the
/// webhook. The token itself never reaches the database (RNF-15); only the
/// reference to the environment variable holding it does.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct EnableBridgeRequest {
    /// Discord channel snowflake this channel mirrors.
    pub discord_channel_id: crate::scalars::Snowflake,
    pub discord_webhook_id: crate::scalars::Snowflake,
    /// Name of the environment variable holding the webhook token,
    /// e.g. `DISCORD_WEBHOOK_GERAL`.
    pub token_ref: String,
}

/// `POST /admin/bridge/reconcile` — result of one RF-31a pass.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReconcileReport {
    #[ts(type = "number")]
    pub channels_examined: i64,
    #[ts(type = "number")]
    pub messages_examined: i64,
    #[ts(type = "number")]
    pub messages_requeued: i64,
    /// Divergences that could not be resolved and need a human.
    pub unresolved: Vec<String>,
}
