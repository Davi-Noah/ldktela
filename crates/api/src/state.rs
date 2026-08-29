//! Shared handler state.

use std::sync::Arc;

use db::PgPool;

use crate::config::Config;
use crate::gateway::Hub;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    /// The gateway session registry. REST mutations publish through it; nothing
    /// that carries content reads its routing index (CLAUDE.md §2.7).
    pub hub: Arc<Hub>,
}

impl AppState {
    pub fn new(pool: PgPool, config: Config) -> Self {
        let hub = Arc::new(Hub::new(config.gateway));
        Self {
            pool,
            config: Arc::new(config),
            hub,
        }
    }
}
