//! Repositories. Every SQL statement in the project lives under this module
//! (CLAUDE.md §7); handlers never carry inline queries.

pub mod categories;
pub mod channels;
pub mod guilds;
pub mod invites;
pub mod messages;
pub mod permissions;
pub mod refresh_tokens;
pub mod roles;
pub mod users;
