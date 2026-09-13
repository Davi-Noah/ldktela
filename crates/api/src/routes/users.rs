//! `/users/*` (`docs/rest-api.md` §6.1).
//!
//! One route. There is no profile editing and no public profile lookup: the
//! profile is Discord's, and everyone you can see is already in a room with you,
//! where the participant payload carries what the UI needs.

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use db::repo::users;
use protocol::user::CurrentUser;

use crate::error::AppError;
use crate::extract::Json;
use crate::middleware::auth::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/users/@me", get(me))
}

#[tracing::instrument(skip(state), fields(user_id = %caller.id))]
async fn me(
    State(state): State<AppState>,
    caller: AuthUser,
) -> Result<Json<CurrentUser>, AppError> {
    let user = users::find_by_id(&state.pool, caller.id).await?;
    Ok(Json(user.to_current()))
}
