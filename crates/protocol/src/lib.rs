//! Wire DTOs shared by the REST API and the WebSocket gateway.
//!
//! This crate is intentionally logic-free: it only describes the shapes that
//! cross the network boundary. TypeScript bindings are generated from here by
//! `just types` (ts-rs), so the frontend never hand-writes a payload type.
//!
//! Two numeric conventions, both load-bearing:
//!
//! * Anything that can exceed 2^53 — permission masks and Discord snowflakes —
//!   travels as a **decimal string** (`docs/api/rest-api.md` §6.4). `Number` in
//!   JavaScript loses precision above that and the bug is silent.
//! * Every other 64-bit field (byte counts, durations, sequence numbers) is
//!   annotated `#[ts(type = "number")]`. `JSON.parse` produces `number`, never
//!   `bigint`, so ts-rs's default `bigint` mapping would describe a value the
//!   client never receives.

pub mod auth;
pub mod bridge;
pub mod channel;
pub mod error;
pub mod gateway;
pub mod guild;
pub mod message;
pub mod page;
pub mod patch;
pub mod scalars;
pub mod search;
pub mod user;
pub mod voice;

pub use error::{ErrorBody, ErrorCode, ErrorResponse, FieldError};
pub use page::{Cursor, Page, PageQuery};
pub use scalars::{PermissionMask, Snowflake, Timestamp};

/// Gateway protocol version carried in the `?v=` query string.
pub const GATEWAY_VERSION: u8 = 1;
