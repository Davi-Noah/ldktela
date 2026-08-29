//! Pure domain rules: permission resolution and value validation.
//!
//! No IO lives here. This crate must never depend on `sqlx`, `axum` or `tokio`.
