//! `/users/*` (`docs/api/rest-api.md` §6.1).

use axum::extract::State;
use axum::routing::{get, patch};
use axum::Router;
use db::repo::users;
use domain::validation::{self, limits, Validation};
use protocol::gateway::DispatchEvent;
use protocol::user::{
    CurrentUser, Presence, UpdateCurrentUserRequest, UpdatePresenceRequest, User,
};
use uuid::Uuid;

use crate::error::AppError;
use crate::extract::{Json, Path};
use crate::middleware::auth::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users/@me", get(me).patch(update_me))
        .route("/users/{id}", get(profile))
        .route("/users/@me/presence", patch(update_presence))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn me(
    State(state): State<AppState>,
    caller: AuthUser,
) -> Result<Json<CurrentUser>, AppError> {
    let user = users::find_by_id(&state.pool, caller.id).await?;
    let status = state.hub.presence_of(caller.id, caller.id).await;
    Ok(Json(user.to_current(status)))
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
    let status = state.hub.presence_of(caller.id, caller.id).await;
    Ok(Json(user.to_current(status)))
}

/// `PATCH /users/@me/presence`. The only source of `idle`, `dnd` and
/// `invisible`; `online` and `offline` derive from the heartbeat.
#[tracing::instrument(skip(state, body), fields(user_id = %caller.id))]
async fn update_presence(
    State(state): State<AppState>,
    caller: AuthUser,
    Json(body): Json<UpdatePresenceRequest>,
) -> Result<Json<Presence>, AppError> {
    if !body.status.is_settable() {
        return Err(AppError::Validation(vec![protocol::error::FieldError {
            field: "status".into(),
            code: "NOT_ALLOWED".into(),
        }]));
    }
    state.hub.declare_presence(caller.id, body.status).await;

    // Third parties never see `invisible` (RF-04), so the broadcast carries the
    // masked value while the response to the user carries the real one.
    let public = state.hub.presence_of(caller.id, Uuid::nil()).await;
    for guild in db::repo::guilds::list_for_user(&state.pool, caller.id).await? {
        state
            .hub
            .publish_to_guild(
                &state.pool,
                guild.id,
                DispatchEvent::PresenceUpdate(Presence {
                    user_id: caller.id,
                    status: public,
                }),
            )
            .await;
    }
    Ok(Json(Presence {
        user_id: caller.id,
        status: state.hub.presence_of(caller.id, caller.id).await,
    }))
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
