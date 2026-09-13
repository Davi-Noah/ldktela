//! Authentication: token issuance and the refresh rotation with family reuse
//! detection (RF-02).
//!
//! There is no password module any more. Identity arrives as a pairing code
//! resolved against Discord (ADR-0009); everything from the token pair onwards
//! is unchanged from v1.

pub mod session;
pub mod token;

pub use session::{issue_session, rotate_refresh};
