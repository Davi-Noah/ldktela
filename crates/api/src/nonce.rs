//! Send idempotency (`docs/api/rest-api.md` §6.5).
//!
//! A client generates a `nonce`, inserts the message optimistically, and resends
//! after a network timeout. Repeating the same nonce in the same channel within
//! 60 seconds must return the message already created, with `200` instead of
//! `201` — otherwise every flaky connection produces a duplicate.
//!
//! Two simultaneous sends of one nonce are the hard case, and a plain
//! "is it already there?" check does not cover it: both callers look, both find
//! nothing, both insert. Each nonce therefore carries a one-permit gate. The
//! first caller holds it while inserting; the second blocks on it and, once it
//! gets in, finds the message the first one recorded.
//!
//! The map lives in memory rather than in a column. The system runs as a single
//! instance (RNF-17), and a 60-second concern does not justify an index and a
//! write on the hot path. The cost is that a restart inside the window can let a
//! duplicate through; a restart also drops every socket, so the client is
//! reconnecting anyway.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::{Duration, Instant};
use uuid::Uuid;

/// The idempotency window from §6.5.
pub const WINDOW: Duration = Duration::from_secs(60);

/// Nonces are client-supplied strings; the cap stops one from being used as
/// unbounded server memory.
const MAX_NONCE_LEN: usize = 64;

type Key = (Uuid, String);

#[derive(Clone)]
struct Slot {
    /// One permit: whoever holds it owns the insert for this nonce.
    gate: Arc<Semaphore>,
    message_id: Option<Uuid>,
    recorded: Instant,
}

/// Proof that the holder owns the insert for a nonce. Keeping it alive is what
/// makes a concurrent sender wait instead of inserting a duplicate.
pub struct Claimed {
    key: Key,
    _permit: OwnedSemaphorePermit,
}

/// What a claim on a nonce resolved to.
pub enum Claim {
    /// The caller owns this nonce and should create the message.
    Fresh(Claimed),
    /// The message already exists; return it with `200`.
    Existing(Uuid),
}

#[derive(Default)]
pub struct NonceRegistry {
    entries: Mutex<HashMap<Key, Slot>>,
}

impl NonceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claims `(channel, nonce)`, waiting for a concurrent sender to finish
    /// rather than racing it.
    pub async fn claim(&self, channel_id: Uuid, nonce: &str) -> Claim {
        let key: Key = (channel_id, nonce.to_owned());

        let gate = {
            let mut entries = self.entries.lock().await;
            Self::evict_expired(&mut entries);
            let slot = entries.entry(key.clone()).or_insert_with(|| Slot {
                gate: Arc::new(Semaphore::new(1)),
                message_id: None,
                recorded: Instant::now(),
            });
            if let Some(id) = slot.message_id {
                return Claim::Existing(id);
            }
            slot.gate.clone()
        };

        // Blocks only while another caller is mid-insert for this exact nonce.
        let permit = gate
            .acquire_owned()
            .await
            .expect("the nonce gate is never closed");

        // The winner may have finished while this caller waited.
        if let Some(slot) = self.entries.lock().await.get(&key) {
            if let Some(id) = slot.message_id {
                return Claim::Existing(id);
            }
        }
        Claim::Fresh(Claimed {
            key,
            _permit: permit,
        })
    }

    /// Records which message the claim produced, and releases the gate.
    pub async fn fulfil(&self, claimed: Claimed, message_id: Uuid) {
        let mut entries = self.entries.lock().await;
        if let Some(slot) = entries.get_mut(&claimed.key) {
            slot.message_id = Some(message_id);
            slot.recorded = Instant::now();
        }
        drop(entries);
        drop(claimed);
    }

    /// Releases a claim whose insert failed, so a retry is not told the message
    /// exists when it does not.
    pub async fn release(&self, claimed: Claimed) {
        let mut entries = self.entries.lock().await;
        if entries
            .get(&claimed.key)
            .is_some_and(|slot| slot.message_id.is_none())
        {
            entries.remove(&claimed.key);
        }
        drop(entries);
        drop(claimed);
    }

    fn evict_expired(entries: &mut HashMap<Key, Slot>) {
        let now = Instant::now();
        entries.retain(|_, slot| {
            // A slot whose gate is taken is mid-insert; expiring it would let a
            // second caller through.
            slot.gate.available_permits() == 0 || now.duration_since(slot.recorded) < WINDOW
        });
    }

    #[cfg(test)]
    async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }
}

/// A nonce the server is willing to remember.
pub fn is_acceptable(nonce: &str) -> bool {
    !nonce.is_empty() && nonce.len() <= MAX_NONCE_LEN
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_fresh(claim: Claim) -> Claimed {
        match claim {
            Claim::Fresh(claimed) => claimed,
            Claim::Existing(id) => panic!("esperava Fresh, veio Existing({id})"),
        }
    }

    fn expect_existing(claim: Claim) -> Uuid {
        match claim {
            Claim::Existing(id) => id,
            Claim::Fresh(_) => panic!("esperava Existing, veio Fresh"),
        }
    }

    #[tokio::test]
    async fn the_second_send_of_a_nonce_resolves_to_the_first_message() {
        let registry = NonceRegistry::new();
        let channel = Uuid::now_v7();
        let message = Uuid::now_v7();

        let claimed = expect_fresh(registry.claim(channel, "abc").await);
        registry.fulfil(claimed, message).await;
        assert_eq!(
            expect_existing(registry.claim(channel, "abc").await),
            message
        );
    }

    #[tokio::test]
    async fn a_concurrent_sender_waits_and_then_finds_the_message() {
        let registry = Arc::new(NonceRegistry::new());
        let channel = Uuid::now_v7();
        let message = Uuid::now_v7();

        // O primeiro reivindica e segura o portão, simulando um insert em curso.
        let claimed = expect_fresh(registry.claim(channel, "abc").await);

        let waiter = {
            let registry = registry.clone();
            tokio::spawn(async move { registry.claim(channel, "abc").await })
        };
        // Dá tempo de o segundo bloquear no portão.
        tokio::time::sleep(Duration::from_millis(30)).await;
        registry.fulfil(claimed, message).await;

        let resolved = expect_existing(waiter.await.unwrap());
        assert_eq!(
            resolved, message,
            "quem chegou depois precisa esperar e receber a mensagem do primeiro"
        );
    }

    #[tokio::test]
    async fn a_concurrent_sender_proceeds_when_the_first_insert_failed() {
        let registry = Arc::new(NonceRegistry::new());
        let channel = Uuid::now_v7();
        let claimed = expect_fresh(registry.claim(channel, "abc").await);

        let waiter = {
            let registry = registry.clone();
            tokio::spawn(async move { registry.claim(channel, "abc").await })
        };
        tokio::time::sleep(Duration::from_millis(30)).await;
        registry.release(claimed).await;

        let _ = expect_fresh(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn a_nonce_is_scoped_to_its_channel() {
        let registry = NonceRegistry::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let claimed = expect_fresh(registry.claim(a, "abc").await);
        registry.fulfil(claimed, Uuid::now_v7()).await;
        let _ = expect_fresh(registry.claim(b, "abc").await);
    }

    #[tokio::test]
    async fn a_failed_insert_releases_the_claim() {
        let registry = NonceRegistry::new();
        let channel = Uuid::now_v7();
        let claimed = expect_fresh(registry.claim(channel, "abc").await);
        registry.release(claimed).await;
        assert_eq!(registry.len().await, 0);
        let _ = expect_fresh(registry.claim(channel, "abc").await);
    }

    #[tokio::test(start_paused = true)]
    async fn an_entry_older_than_the_window_stops_deduplicating() {
        let registry = NonceRegistry::new();
        let channel = Uuid::now_v7();
        let claimed = expect_fresh(registry.claim(channel, "abc").await);
        registry.fulfil(claimed, Uuid::now_v7()).await;

        tokio::time::advance(WINDOW + Duration::from_secs(1)).await;
        let _ = expect_fresh(registry.claim(channel, "abc").await);
    }

    #[test]
    fn an_empty_or_oversized_nonce_is_refused() {
        assert!(is_acceptable("01J8XQ"));
        assert!(!is_acceptable(""));
        assert!(!is_acceptable(&"a".repeat(MAX_NONCE_LEN + 1)));
    }
}
