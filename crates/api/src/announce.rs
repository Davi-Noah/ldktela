//! What the Discord side needs to know about a room, as it changes (S8).
//!
//! The dependency arrow points from `bot` to `api` and never back: the bot is a
//! producer of Discord events, not a service this crate calls. So instead of
//! reaching for serenity, room changes are published here and the bot subscribes
//! — which also keeps this crate free of any opinion about *how* a room gets
//! announced.
//!
//! Each message is a **whole snapshot**, not a delta. That is what lets the bot
//! keep one message per session and edit it (the S8 acceptance criterion): a
//! consumer that missed an event still converges, and coalescing two rapid
//! changes into one edit is just dropping the older snapshot.

use tokio::sync::broadcast;

/// The public state of one room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomBroadcast {
    pub discord_channel_id: i64,
    /// Discord ids of whoever is publishing, in join order.
    pub publishers: Vec<i64>,
    /// People in the room who are not publishing.
    pub viewers: usize,
}

impl RoomBroadcast {
    pub fn is_idle(&self) -> bool {
        self.publishers.is_empty()
    }
}

/// Fan-out of room snapshots to whoever is listening.
///
/// Lossy on purpose. A slow consumer that falls behind gets `Lagged` and skips
/// to the newest snapshot, which is exactly right for state that is only ever
/// "how things are now" — blocking the media path to deliver an obsolete
/// audience count would be the wrong trade.
pub struct Announcer {
    tx: broadcast::Sender<RoomBroadcast>,
}

impl Announcer {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RoomBroadcast> {
        self.tx.subscribe()
    }

    /// Publishes a snapshot. Silent when nobody is listening, which is the
    /// normal state of a server running without the bot connected.
    pub fn publish(&self, broadcast: RoomBroadcast) {
        let _ = self.tx.send(broadcast);
    }
}

impl Default for Announcer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(publishers: Vec<i64>, viewers: usize) -> RoomBroadcast {
        RoomBroadcast {
            discord_channel_id: 1,
            publishers,
            viewers,
        }
    }

    #[tokio::test]
    async fn a_subscriber_sees_the_snapshots_in_order() {
        let announcer = Announcer::new();
        let mut rx = announcer.subscribe();

        announcer.publish(snapshot(vec![10], 0));
        announcer.publish(snapshot(vec![10], 2));

        assert_eq!(rx.recv().await.expect("primeira").viewers, 0);
        assert_eq!(rx.recv().await.expect("segunda").viewers, 2);
    }

    #[tokio::test]
    async fn publishing_with_nobody_listening_is_not_an_error() {
        // O servidor roda sem o bot conectado em desenvolvimento, e a sala nao
        // pode falhar por causa disso.
        let announcer = Announcer::new();
        announcer.publish(snapshot(vec![10], 0));
    }

    #[test]
    fn a_room_with_no_publisher_is_idle() {
        // E o sinal de que a sessao acabou e a mensagem deve ser encerrada.
        assert!(snapshot(Vec::new(), 3).is_idle());
        assert!(!snapshot(vec![10], 0).is_idle());
    }
}
