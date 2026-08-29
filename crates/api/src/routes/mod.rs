//! REST routes. One module per section of `docs/api/rest-api.md` §6.

pub mod attachments;
pub mod auth;
pub mod channels;
pub mod dms;
pub mod guilds;
pub mod health;
pub mod invites;
pub mod messages;
pub mod search;
pub mod users;
pub mod voice;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(attachments::router())
        .merge(users::router())
        .merge(invites::router())
        .merge(guilds::router())
        .merge(channels::router())
        .merge(messages::router())
        .merge(dms::router())
        .merge(search::router())
        .merge(voice::router())
}
