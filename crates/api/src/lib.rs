//! HTTP surface: REST routes, WebSocket gateway and middleware.

pub mod announce;
pub mod auth;
pub mod config;
pub mod discord;
pub mod error;
pub mod extract;
pub mod gateway;
pub mod jobs;
pub mod livekit;
pub mod middleware;
pub mod permissions;
pub mod routes;
pub mod state;

use axum::http::{header, HeaderValue, Method};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;

pub use error::{AppError, UpstreamError};
pub use state::AppState;

/// Origins the desktop client can present.
///
/// The WebView is always a different origin from the API, so every request the
/// client makes is cross-origin and needs this — without it the browser refuses
/// the request before it leaves the machine, and the client sees a network
/// failure with nothing in the server log to explain it.
///
/// Listed explicitly rather than allowing anything: the list is short, closed,
/// and knowable. Nothing here relies on `*` being safe.
const ALLOWED_ORIGINS: [&str; 5] = [
    // Vite, em `just app`.
    "http://localhost:1420",
    "http://127.0.0.1:1420",
    // WebView2 no Windows, que e a plataforma suportada (RNF-10).
    "http://tauri.localhost",
    "https://tauri.localhost",
    // macOS/Linux, que nao sao suportados mas tambem nao custam nada aqui.
    "tauri://localhost",
];

fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(
            ALLOWED_ORIGINS
                .iter()
                .filter_map(|o| HeaderValue::from_str(o).ok())
                .collect::<Vec<_>>(),
        )
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

/// Base path of every REST route (`docs/rest-api.md`).
pub const API_BASE: &str = "/api/v1";

/// Largest JSON body accepted.
///
/// The biggest legitimate request in this product is a pairing code, so this is
/// three orders of magnitude of headroom. It exists to make an absurd body an
/// early rejection rather than an allocation.
const MAX_BODY_BYTES: usize = 16 * 1024;

/// Builds the full HTTP router. `server` owns the listener; `api` owns the shape.
pub fn router(state: AppState) -> Router {
    Router::new()
        .nest(API_BASE, routes::router())
        .merge(gateway::router())
        .layer(cors())
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(axum::middleware::from_fn(middleware::request_id::propagate))
        .with_state(state)
}
