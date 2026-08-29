//! Authentication (`docs/api/rest-api.md` §6.1, RF-01, RF-01a).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::user::CurrentUser;

/// `POST /auth/register`. Consumes the invite in the same transaction.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct RegisterRequest {
    pub invite_code: String,
    pub email: String,
    pub username: String,
    pub password: String,
}

/// `POST /auth/login`.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// `POST /auth/refresh` and `POST /auth/logout`.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// The token pair. The refresh token is opaque and is stored by the Rust core in
/// the Windows credential vault — never in `localStorage` (RF-01b).
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

/// Claims carried by the access token (`docs/api/rest-api.md` §2).
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
