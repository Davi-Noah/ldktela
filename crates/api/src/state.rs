//! Shared handler state.

use std::sync::Arc;

use db::PgPool;

use crate::config::Config;
use crate::gateway::Hub;
use crate::nonce::NonceRegistry;
use crate::storage::Storage;
use crate::voice::Voice;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    /// The gateway session registry. REST mutations publish through it; nothing
    /// that carries content reads its routing index (CLAUDE.md §2.7).
    pub hub: Arc<Hub>,
    /// Send idempotency for the 60 s window of rest-api.md 6.5.
    pub nonces: Arc<NonceRegistry>,
    /// Object storage. Bytes never pass through here (RF-10); this signs
    /// URLs, confirms objects exist and collects orphans.
    pub storage: Arc<Storage>,
    /// LiveKit: room tokens, the camera guard and webhook verification.
    pub voice: Arc<Voice>,
}

impl AppState {
    pub fn new(pool: PgPool, config: Config) -> Self {
        let hub = Arc::new(Hub::new(config.gateway));
        let storage = Arc::new(Storage::new(&config.storage));
        let voice = Arc::new(Voice::new(config.voice.clone()));
        Self {
            pool,
            config: Arc::new(config),
            hub,
            nonces: Arc::new(NonceRegistry::new()),
            storage,
            voice,
        }
    }
}
