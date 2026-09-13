//! Pure domain rules: permission resolution and value validation.
//!
//! No IO lives here. This crate must never depend on `sqlx`, `axum` or `tokio`.

pub mod discord;
pub mod pairing;
pub mod validation;

pub use discord::{
    resolve, ChannelRef, DiscordPermissions, GuildRef, MemberRef, Overwrite, OverwriteKind, RoleRef,
};
pub use validation::{FieldError, Validation, ValidationCode};
