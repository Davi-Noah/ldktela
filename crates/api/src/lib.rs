//! HTTP surface: REST routes, WebSocket gateway and middleware.

pub mod auth;
pub mod config;
pub mod error;
pub mod extract;
pub mod gateway;
pub mod jobs;
pub mod middleware;
pub mod nonce;
pub mod permissions;
pub mod routes;
pub mod state;
pub mod storage;

use axum::Router;
use tower_http::limit::RequestBodyLimitLayer;

pub use error::{AppError, UpstreamError};
pub use state::AppState;

/// Base path of every REST route (`docs/api/rest-api.md`).
pub const API_BASE: &str = "/api/v1";

/// Largest JSON body accepted. Attachments never pass through the backend
/// (RF-10), so no legitimate request needs more.
const MAX_BODY_BYTES: usize = 256 * 1024;

/// Builds the full HTTP router. `server` owns the listener; `api` owns the shape.
pub fn router(state: AppState) -> Router {
    Router::new()
        .nest(API_BASE, routes::router())
        .merge(gateway::router())
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(axum::middleware::from_fn(middleware::request_id::propagate))
        .with_state(state)
}
