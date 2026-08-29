//! Extractors that fail through `AppError`.
//!
//! `axum::Json` rejects a body with its own response: `422` for a valid JSON
//! document of the wrong shape, `400` for a syntax error, plain text in both.
//! The contract has neither a `422` nor a second error shape
//! (`docs/api/rest-api.md` §3), so the extractors are wrapped here and every
//! rejection becomes an `AppError`.
//!
//! This matters beyond tidiness: a permission mask sent as a JSON number instead
//! of a decimal string is exactly the mistake §6.4 warns about, and it has to
//! come back as `400 VALIDATION_FAILED` naming the field — not as a `422` the
//! client has no handler for.

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use protocol::error::FieldError;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::AppError;

/// `axum::Json` with contract-shaped rejections.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json<T>(pub T);

impl<S, T> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(request, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(map_json_rejection(&rejection)),
        }
    }
}

/// Responses use the same wrapper, so a handler never mixes `axum::Json` on the
/// way out with `crate::extract::Json` on the way in.
impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

fn map_json_rejection(rejection: &JsonRejection) -> AppError {
    match rejection {
        // The body was larger than the configured limit before it was parsed.
        JsonRejection::BytesRejection(_) => AppError::PayloadTooLarge,
        other => {
            let detail = other.body_text();
            // Kept out of the response: it is serde's wording, in English, and
            // the contract requires a Portuguese, displayable message.
            tracing::debug!(detail = %detail, "rejected request body");
            AppError::Validation(vec![FieldError {
                field: field_from_serde_message(&detail),
                code: "INVALID_FORMAT".into(),
            }])
        }
    }
}

/// Pulls the field name out of serde's path-annotated message.
///
/// axum runs deserialisation through `serde_path_to_error`, so the message
/// starts with the JSON pointer to the offending field, e.g.
/// `Failed to deserialize the JSON body into the target type: allow: invalid
/// type: integer …`. Anything unrecognised falls back to `body`.
fn field_from_serde_message(message: &str) -> String {
    let tail = message
        .split_once("target type: ")
        .map(|(_, rest)| rest)
        .unwrap_or(message);
    let candidate = tail.split(':').next().unwrap_or_default().trim();
    let looks_like_a_path = !candidate.is_empty()
        && candidate.len() <= 64
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '[' | ']'));
    if looks_like_a_path {
        candidate.to_string()
    } else {
        "body".to_string()
    }
}

/// `axum::extract::Path` with contract-shaped rejections.
///
/// A malformed id in the path names a resource that cannot exist, so it answers
/// `404` — the same as an id that exists but is invisible. Answering `400` would
/// tell a prober that their id shape was the only thing wrong.
#[derive(Debug, Clone, Copy, Default)]
pub struct Path<T>(pub T);

impl<S, T> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(Self(value)),
            Err(rejection) => {
                if let PathRejection::MissingPathParams(err) = &rejection {
                    // A route declared parameters the handler does not read.
                    // That is a wiring bug, not client input.
                    return Err(AppError::Internal(anyhow::anyhow!("path params: {err}")));
                }
                tracing::debug!(detail = %rejection, "rejected path parameter");
                Err(AppError::invisible("resource"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_name_is_recovered_from_serdes_path() {
        assert_eq!(
            field_from_serde_message(
                "Failed to deserialize the JSON body into the target type: \
                 allow: invalid type: integer `256`, expected a string at line 1 column 20"
            ),
            "allow"
        );
        assert_eq!(
            field_from_serde_message(
                "Failed to deserialize the JSON body into the target type: \
                 positions[0].id: invalid length"
            ),
            "positions[0].id"
        );
    }

    #[test]
    fn an_unrecognised_message_falls_back_to_body_instead_of_leaking_it() {
        assert_eq!(
            field_from_serde_message("Expected request with `Content-Type: application/json`"),
            "body"
        );
        assert_eq!(
            field_from_serde_message("EOF while parsing a value at line 1 column 0"),
            "body"
        );
    }
}
