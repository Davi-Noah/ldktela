//! The `Authorization: Bearer` extractor.
//!
//! Every route except `/auth/*` and `/health` takes an `AuthUser`
//! (`docs/api/rest-api.md` §2). Extraction failure is an `AppError`, so the
//! error body is the same one shape as everywhere else.

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::auth::token::{self, TokenError};
use crate::error::AppError;
use crate::state::AppState;

/// The authenticated caller.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub id: Uuid,
    /// Token id, for revocation and for correlating a session in the log.
    pub jti: Uuid,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::Unauthorized)?;
        let presented = header
            .strip_prefix("Bearer ")
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or(AppError::Unauthorized)?;

        let claims = token::verify_access(state.config.jwt_signing_key.as_bytes(), presented)
            .map_err(|e| match e {
                // The two are distinct on purpose: the client refreshes once on
                // TOKEN_EXPIRED and logs out on UNAUTHENTICATED.
                TokenError::Expired => AppError::TokenExpired,
                TokenError::Invalid => AppError::Unauthorized,
            })?;

        let id = claims
            .sub
            .parse::<Uuid>()
            .map_err(|_| AppError::Unauthorized)?;
        let jti = claims
            .jti
            .parse::<Uuid>()
            .map_err(|_| AppError::Unauthorized)?;
        Ok(Self { id, jti })
    }
}

/// The `User-Agent`, recorded against the refresh token so a user can tell their
/// sessions apart. Truncated: it is free-form client input.
pub fn user_agent(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(200).collect())
}
