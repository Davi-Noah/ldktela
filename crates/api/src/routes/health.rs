use axum::{routing::get, Json, Router};
use serde::Serialize;

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    version: &'static str,
}

pub fn router() -> Router {
    Router::new().route("/health", get(health))
}

/// Liveness probe. Deliberately does not touch the database: a probe that fails
/// on a transient pool hiccup causes restart loops.
#[tracing::instrument]
async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}
