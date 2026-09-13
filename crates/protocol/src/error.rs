//! The single error body shape (`docs/rest-api.md` §3). There is no other.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Wrapper so every error response is `{ "error": { … } }`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

impl ErrorResponse {
    pub fn new(body: ErrorBody) -> Self {
        Self { error: body }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorBody {
    /// `SCREAMING_SNAKE_CASE`, stable, used in client logic.
    pub code: String,
    /// Portuguese, displayable. Never carries internal detail.
    pub message: String,
    /// Only present on `VALIDATION_FAILED`.
    pub details: Option<Vec<FieldError>>,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FieldError {
    pub field: String,
    pub code: String,
}

/// Every `code` the API can return (`docs/rest-api.md` §3, plus the
/// domain-specific codes named elsewhere in the contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ValidationFailed,
    Unauthenticated,
    TokenExpired,
    TokenReused,
    Forbidden,
    NotFound,
    Conflict,
    /// The body exceeded the request limit. No legitimate client can produce
    /// one — the largest body in this product is a pairing code — but the
    /// extractor can still reject, and every rejection needs a code.
    PayloadTooLarge,
    RateLimited,
    Internal,
    /// The room already holds the maximum number of publishers (RNF-05).
    RoomCapacity,
    /// The Discord replica is too far behind to vouch for access, so the
    /// request fails closed (RF-09).
    ReplicaStale,
    /// An upstream dependency failed (LiveKit, Discord).
    UpstreamFailure,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::TokenExpired => "TOKEN_EXPIRED",
            Self::TokenReused => "TOKEN_REUSED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            Self::RateLimited => "RATE_LIMITED",
            Self::Internal => "INTERNAL",
            Self::RoomCapacity => "ROOM_CAPACITY",
            Self::ReplicaStale => "REPLICA_STALE",
            Self::UpstreamFailure => "UPSTREAM_FAILURE",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_body_matches_the_documented_shape() {
        let body = ErrorResponse::new(ErrorBody {
            code: ErrorCode::Forbidden.to_string(),
            message: "Você não tem permissão para enviar mensagens neste canal.".into(),
            details: None,
            request_id: "01J8XQ".into(),
        });
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["error"]["code"], "FORBIDDEN");
        assert!(json["error"]["details"].is_null());
        assert_eq!(json["error"]["request_id"], "01J8XQ");
        assert_eq!(
            json.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["error"]
        );
    }

    #[test]
    fn every_code_serialises_in_screaming_snake_case() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::PayloadTooLarge).unwrap(),
            "\"PAYLOAD_TOO_LARGE\""
        );
        assert_eq!(ErrorCode::RoomCapacity.as_str(), "ROOM_CAPACITY");
        assert_eq!(ErrorCode::ReplicaStale.as_str(), "REPLICA_STALE");
        // O `as_str` e o rename do serde precisam concordar: o cliente usa o
        // primeiro em log e o segundo em logica.
        assert_eq!(
            serde_json::to_string(&ErrorCode::ReplicaStale).unwrap(),
            format!("\"{}\"", ErrorCode::ReplicaStale.as_str())
        );
    }
}
