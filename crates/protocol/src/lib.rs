//! Wire DTOs shared by the REST API and the WebSocket gateway.
//!
//! This crate is intentionally logic-free: it only describes the shapes that
//! cross the network boundary. TypeScript bindings are generated from here by
//! `just types` (ts-rs), so the frontend never hand-writes a payload type.

/// Gateway protocol version carried in the `?v=` query string.
pub const GATEWAY_VERSION: u8 = 1;
