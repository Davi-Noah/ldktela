//! Users (RF-04).
//!
//! Every field here is a mirror of the Discord profile. There is no profile
//! editing in this product: the place to change your name and avatar is Discord.
//!
//! There is no presence status either. Online/idle/dnd is something Discord
//! already shows, next to this window (CLAUDE.md §2.1).

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::scalars::{Snowflake, Timestamp};

/// A user as seen by others in a room.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UserSummary {
    pub id: Uuid,
    pub discord_user_id: Snowflake,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

/// `GET /users/@me`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CurrentUser {
    pub id: Uuid,
    pub discord_user_id: Snowflake,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: Timestamp,
}
