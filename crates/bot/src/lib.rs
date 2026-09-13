//! Discord bot: identity pairing, the authorization replica, and room presence.
//!
//! This crate is where serenity lives. It depends on `api` rather than the other
//! way round: the bot is a *producer* of events (voice state changes, membership
//! changes, pairing codes) that `api` consumes, so the arrow points at the
//! consumer's state.

pub mod handler;
pub mod pairing;
pub mod replica_sync;
pub mod revoke;

use anyhow::Context as _;
use api::AppState;
use serenity::prelude::*;

/// Connects the bot and runs until the process stops.
///
/// The intents are the minimum the product needs: guild structure, members (for
/// role resolution) and voice states. **`MESSAGE_CONTENT` is deliberately
/// absent** — we do not read messages, and it is the hardest intent to justify
/// to Discord's review.
pub async fn run(state: AppState) -> anyhow::Result<()> {
    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS | GatewayIntents::GUILD_VOICE_STATES;

    let mut client = Client::builder(&state.config.discord.bot_token, intents)
        .event_handler(handler::Handler::new(state))
        .await
        .context("building the discord client")?;

    client.start().await.context("running the discord client")
}
