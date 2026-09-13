//! The local replica of Discord's authorization state (ADR-0010).
//!
//! Discord is the authority on who may see and join a room. Asking its REST API
//! on every check would put a rate limit in the hot path and make a Discord
//! outage a total outage, so the bot mirrors the state over the gateway and
//! every decision is computed locally against that mirror.
//!
//! The mirror lives in memory and nowhere else. It is rebuilt from
//! `GUILD_CREATE` on each connection, which makes it correct by construction
//! after a restart — a persisted copy would only give us a second version of
//! the truth to disagree with.

pub mod replica;

pub use replica::{ChannelData, GuildData, Replica, Staleness};
