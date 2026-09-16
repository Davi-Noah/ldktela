//! Repositories. Every SQL statement in the project lives under this module
//! (CLAUDE.md §7); handlers never carry inline queries.

pub mod live_tags;
pub mod pairing;
pub mod presence;
pub mod refresh_tokens;
pub mod sessions;
pub mod users;
