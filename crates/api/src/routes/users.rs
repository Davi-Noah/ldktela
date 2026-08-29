//! `/users/*` (`docs/api/rest-api.md` §6.1).

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use db::repo::users;
use domain::validation::{self, limits, Validation};
use protocol::user::{CurrentUser, PresenceStatus, UpdateCurrentUserRequest, User};
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users/@me", get(me).patch(update_me))
        .route("/users/{id}", get(profile))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn me(
    State(state): State<AppState>,
    caller: AuthUser,
) -> Result<Json<CurrentUser>, AppError> {
    let user = users::find_by_id(&state.pool, caller.id).await?;
    // Presence lives in the gateway; until E6 wires it in, a REST read reports
    // the durable part of the profile and `offline`.
    Ok(Json(user.to_current(PresenceStatus::Offline)))
}

#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn update_me(
    State(state): State<AppState>,
    caller: AuthUser,
    Json(body): Json<UpdateCurrentUserRequest>,
) -> Result<Json<CurrentUser>, AppError> {
    let mut v = Validation::new();
    if let Some(Some(value)) = &body.display_name {
        v.check(
            "display_name",
            validation::bounded(value, 1, limits::DISPLAY_NAME_MAX),
        );
    }
    if let Some(Some(value)) = &body.bio {
        v.check(
            "bio",
            validation::optional_max(Some(value), limits::BIO_MAX),
        );
    }
    if let Some(Some(value)) = &body.accent_color {
        v.check("accent_color", validation::hex_color(value));
    }
    if let Some(Some(value)) = &body.avatar_url {
        v.check("avatar_url", validation::bounded(value, 1, 2048));
    }
    v.finish()?;

    let user = users::update_profile(
        &state.pool,
        caller.id,
        body.display_name,
        body.avatar_url,
        body.bio,
        body.accent_color,
    )
    .await?;
    Ok(Json(user.to_current(PresenceStatus::Offline)))
}

/// Public profile. Any authenticated user may read any profile: the community is
/// closed, and hiding profiles from members buys nothing.
#[tracing::instrument(skip(state), fields(caller = %caller.id))]
async fn profile(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<User>, AppError> {
    let user = users::find_by_id(&state.pool, id).await?;
    Ok(Json(user.to_public()))
}
