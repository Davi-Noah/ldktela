//! `POST /attachments/presign` (`docs/api/rest-api.md` §6.6, RF-10, RF-11a).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use domain::validation::{self, limits, Validation};
use domain::Permissions;
use protocol::message::{PresignRequest, PresignResponse};

use crate::error::AppError;
use crate::extract::Json;
use crate::middleware::auth::AuthUser;
use crate::permissions::require_channel;
use crate::state::AppState;
use crate::storage::Storage;

pub fn router() -> Router<AppState> {
    Router::new().route("/attachments/presign", post(presign))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn presign(
    State(state): State<AppState>,
    caller: AuthUser,
    Json(body): Json<PresignRequest>,
) -> Result<(StatusCode, Json<PresignResponse>), AppError> {
    // Permission first: an invisible channel must not even reveal that the file
    // policy exists.
    require_channel(
        &state,
        caller.id,
        body.channel_id,
        Permissions::ATTACH_FILES,
    )
    .await?;

    // RF-11a **before** signing. Signing and then refusing would hand out a URL
    // that was never supposed to exist, and it stays valid for its whole TTL.
    let mut v = Validation::new();
    v.check(
        "filename",
        validation::bounded(&body.filename, 1, limits::EMOJI_MAX.max(255)),
    );
    v.finish()?;

    if let Err(code) = validation::attachment(
        body.size_bytes,
        &body.content_type,
        state.config.max_attachment_bytes,
        &state.config.allowed_content_types,
    ) {
        // A file over the size limit is the one case the contract gives its own
        // status: 413, not a validation error (§3).
        if code == domain::ValidationCode::TooLong {
            return Err(AppError::PayloadTooLarge);
        }
        return Err(AppError::Validation(vec![protocol::error::FieldError {
            field: if code == domain::ValidationCode::NotAllowed {
                "content_type".into()
            } else {
                "size_bytes".into()
            },
            code: code.as_str().to_string(),
        }]));
    }

    let key = Storage::key_for(&body.filename);
    let upload_url = state
        .storage
        .presign_put(&key, &body.content_type, body.size_bytes)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(PresignResponse {
            r2_key: key,
            upload_url,
            expires_in: state.storage.presign_ttl_seconds(),
        }),
    ))
}
