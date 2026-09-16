//! Shared handler state.

use std::sync::Arc;

use db::PgPool;

use crate::announce::Announcer;
use crate::config::Config;
use crate::discord::Replica;
use crate::gateway::Hub;
use crate::livekit::Rooms;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    /// The gateway session registry. Everything that reaches a client goes
    /// through it, and the recipient set is always computed per room.
    pub hub: Arc<Hub>,
    /// LiveKit: room tokens, the publisher guard and webhook verification.
    pub rooms: Arc<Rooms>,
    /// The Discord mirror that answers every authorization question.
    ///
    /// Shared with the bot, which is the only writer. Handlers only read.
    pub replica: Arc<Replica>,
    /// Room snapshots for the Discord side to announce (S8).
    ///
    /// Written here, read by the bot. The arrow stays pointed at the consumer.
    pub announce: Arc<Announcer>,
}

impl AppState {
    pub fn new(pool: PgPool, config: Config) -> Self {
        let hub = Arc::new(Hub::new(config.gateway));
        let rooms = Arc::new(Rooms::new(config.rooms.clone()));
        let replica = Arc::new(Replica::new(config.discord.replica_grace));
        Self {
            pool,
            config: Arc::new(config),
            hub,
            rooms,
            replica,
            announce: Arc::new(Announcer::new()),
        }
    }
}
