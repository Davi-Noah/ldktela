//! REST routes. One module per section of `docs/api/rest-api.md` §6.

pub mod auth;
pub mod health;
pub mod private_calls;
pub mod rooms;
pub mod users;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(users::router())
        .merge(private_calls::router())
        .merge(rooms::router())
}
