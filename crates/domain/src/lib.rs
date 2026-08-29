//! Pure domain rules: permission resolution and value validation.
//!
//! No IO lives here. This crate must never depend on `sqlx`, `axum` or `tokio`.

pub mod mentions;
pub mod permissions;
pub mod resolve;
pub mod validation;

pub use mentions::Mentions;
pub use permissions::Permissions;
pub use resolve::{resolve, GuildContext, Overwrite, PermissionContext};
pub use validation::{FieldError, Validation, ValidationCode};
