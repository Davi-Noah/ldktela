//! Authentication (RF-01, RF-02).
//!
//! There is no registration and no login: the Discord account is the identity,
//! reached through a single-use pairing code issued by the bot (ADR-0009). Only
//! the token half of the v1 flow survived, and it survived unchanged.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::user::CurrentUser;

/// `POST /auth/pair`. The code was handed to the user by the bot, ephemerally,
/// inside Discord.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct PairRequest {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OAuthStartResponse {
    pub authorize_url: String,
    pub attempt_id: Uuid,
    pub poll_secret: String,
    #[ts(type = "number")]
    pub expires_in: i64,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct OAuthCompleteRequest {
    pub attempt_id: Uuid,
    pub poll_secret: String,
}

/// `POST /auth/refresh` and `POST /auth/logout`.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// The token pair. The refresh token is opaque and is stored by the Rust core in
/// the Windows credential vault — never in `localStorage` (RF-03).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// Access token lifetime in seconds.
    #[ts(type = "number")]
    pub expires_in: i64,
    pub user: CurrentUser,
}

/// Claims carried by the access token.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AccessTokenClaims {
    /// User id, as a UUID string.
    pub sub: String,
    /// Token id, for revocation.
    pub jti: String,
    /// Issued at, seconds since the epoch.
    #[ts(type = "number")]
    pub iat: i64,
    /// Expiry, seconds since the epoch.
    #[ts(type = "number")]
    pub exp: i64,
}
