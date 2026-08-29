//! HTTP surface: REST routes, WebSocket gateway and middleware.

pub mod config;
pub mod routes;

use axum::Router;

/// Base path of every REST route (`docs/api/rest-api.md`).
pub const API_BASE: &str = "/api/v1";

/// Builds the full HTTP router. `server` owns the listener; `api` owns the shape.
pub fn router() -> Router {
    Router::new().nest(API_BASE, routes::router())
}
