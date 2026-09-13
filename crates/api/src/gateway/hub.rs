//! The session registry and the publish surface the rest of the crate calls.
//!
//! Fan-out is always computed per room (CLAUDE.md §2.10). There is no cached
//! routing index any more: the recipient set of a room event is exactly the rows
//! of `room_presence` for that channel, which is one indexed query. v1 cached
//! this because the answer required resolving permissions for every member of a
//! guild; now the answer is a lookup, and a cache would only be a way to be
//! wrong.

use std::collections::HashMap;
use std::sync::Arc;

use db::PgPool;
use protocol::gateway::{ControlFrame, DispatchEvent, Opcode};
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::GatewayConfig;
use crate::gateway::session::{Outbox, Session};

#[derive(Default)]
struct Registry {
    sessions: HashMap<Uuid, Arc<Session>>,
    /// Session ids per user, oldest first, so the connection limit can drop the
    /// oldest (`docs/websocket.md` §7).
    by_user: HashMap<Uuid, Vec<Uuid>>,
}

pub struct Hub {
    config: GatewayConfig,
    registry: RwLock<Registry>,
}

impl Hub {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            registry: RwLock::new(Registry::default()),
        }
    }

    pub fn config(&self) -> GatewayConfig {
        self.config
    }

    // -----------------------------------------------------------------------
    // Sessões
    // -----------------------------------------------------------------------

    /// Builds a session **without** publishing it.
    ///
    /// The two steps are separate so `READY` can be dispatched before the
    /// session becomes reachable: `READY` is the first frame of a session by
    /// contract (websocket.md §3.1), and building it takes queries. Registering
    /// first leaves a window in which another connection's event takes
    /// sequence 1.
    pub fn create_session(&self, user_id: Uuid, outbox: Outbox) -> Arc<Session> {
        Arc::new(Session::new(
            user_id,
            outbox,
            self.config.resume_buffer_size,
        ))
    }

    /// Publishes a session, dropping the user's oldest one if they are already
    /// at the connection limit.
    pub async fn attach(&self, session: Arc<Session>) -> Arc<Session> {
        let user_id = session.user_id;
        let mut registry = self.registry.write().await;
        let owned = registry.by_user.entry(user_id).or_default();
        owned.push(session.id);
        let mut evicted = Vec::new();
        while owned.len() > self.config.max_connections_per_user {
            evicted.push(owned.remove(0));
        }
        for oldest in evicted {
            if let Some(dropped) = registry.sessions.remove(&oldest) {
                dropped.send_text(control(Opcode::RECONNECT));
                dropped.disconnect();
            }
        }
        registry.sessions.insert(session.id, session.clone());
        session
    }

    /// Create and attach in one step, for callers with nothing to send first.
    pub async fn register(&self, user_id: Uuid, outbox: Outbox) -> Arc<Session> {
        let session = self.create_session(user_id, outbox);
        self.attach(session).await
    }

    /// Looks up a session for `RESUME`. The user must match: a session id is a
    /// bearer capability otherwise.
    pub async fn resumable(&self, session_id: Uuid, user_id: Uuid) -> Option<Arc<Session>> {
        let registry = self.registry.read().await;
        let session = registry.sessions.get(&session_id)?;
        (session.user_id == user_id).then(|| session.clone())
    }

    pub async fn forget(&self, session_id: Uuid) {
        let mut registry = self.registry.write().await;
        if let Some(session) = registry.sessions.remove(&session_id) {
            if let Some(owned) = registry.by_user.get_mut(&session.user_id) {
                owned.retain(|id| *id != session_id);
                if owned.is_empty() {
                    registry.by_user.remove(&session.user_id);
                }
            }
        }
    }

    /// Drops sessions whose socket has been gone longer than the TTL.
    /// Returns the users who no longer have any session at all.
    pub async fn sweep_expired(&self) -> Vec<Uuid> {
        let cutoff =
            OffsetDateTime::now_utc() - Duration::milliseconds(self.config.session_ttl_ms as i64);
        let mut registry = self.registry.write().await;
        let expired: Vec<Uuid> = registry
            .sessions
            .values()
            .filter(|s| !s.is_connected() && s.disconnected_at().is_some_and(|at| at <= cutoff))
            .map(|s| s.id)
            .collect();

        let mut touched = Vec::new();
        for id in expired {
            if let Some(session) = registry.sessions.remove(&id) {
                if let Some(owned) = registry.by_user.get_mut(&session.user_id) {
                    owned.retain(|x| *x != id);
                    if owned.is_empty() {
                        registry.by_user.remove(&session.user_id);
                        touched.push(session.user_id);
                    }
                }
            }
        }
        touched
    }

    /// True while the user has at least one attached socket.
    pub async fn is_online(&self, user_id: Uuid) -> bool {
        let registry = self.registry.read().await;
        registry.by_user.get(&user_id).is_some_and(|ids| {
            ids.iter()
                .any(|id| registry.sessions.get(id).is_some_and(|s| s.is_connected()))
        })
    }

    // -----------------------------------------------------------------------
    // Publicação
    // -----------------------------------------------------------------------

    /// Sends to every session of every user in `users`, including the session
    /// that caused the change.
    pub async fn publish_to_users(&self, users: &[Uuid], event: DispatchEvent) {
        let registry = self.registry.read().await;
        for user in users {
            let Some(ids) = registry.by_user.get(user) else {
                continue;
            };
            for id in ids {
                if let Some(session) = registry.sessions.get(id) {
                    session.dispatch(event.clone());
                }
            }
        }
    }

    /// Everyone currently in the room.
    ///
    /// Callers that are about to remove someone capture this first: after the
    /// row is gone there is nobody left to compute.
    pub async fn room_members(&self, pool: &PgPool, discord_channel_id: i64) -> Vec<Uuid> {
        match db::repo::presence::list_by_channel(pool, discord_channel_id).await {
            Ok(rows) => rows.into_iter().map(|r| r.user_id).collect(),
            Err(error) => {
                // Falha fechada: um erro ao calcular destinatarios devolve
                // conjunto vazio, nunca um broadcast.
                tracing::error!(%error, discord_channel_id, "computing room members");
                Vec::new()
            }
        }
    }

    /// Sends to whoever is currently in the room.
    pub async fn publish_to_room(
        &self,
        pool: &PgPool,
        discord_channel_id: i64,
        event: DispatchEvent,
    ) {
        let members = self.room_members(pool, discord_channel_id).await;
        self.publish_to_users(&members, event).await;
    }

    /// Sends only to the user's own sessions.
    pub async fn publish_to_user(&self, user_id: Uuid, event: DispatchEvent) {
        self.publish_to_users(&[user_id], event).await;
    }

    /// Graceful shutdown: every session is told to reconnect, and the TTL gives
    /// the process time to come back (§3.4).
    pub async fn broadcast_reconnect(&self) {
        let registry = self.registry.read().await;
        for session in registry.sessions.values() {
            session.send_text(control(Opcode::RECONNECT));
        }
    }
}

/// A control frame with no payload, pre-serialised.
pub fn control(op: Opcode) -> String {
    serde_json::to_string(&ControlFrame::<()> { op, d: None })
        .unwrap_or_else(|_| format!("{{\"op\":{}}}", op.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::room::{RoomLeave, RoomLeaveReason};
    use protocol::scalars::Snowflake;
    use tokio::sync::mpsc;

    fn config() -> GatewayConfig {
        GatewayConfig {
            heartbeat_interval_ms: 30_000,
            session_ttl_ms: 90_000,
            resume_buffer_size: 500,
            max_connections_per_user: 4,
        }
    }

    fn some_event() -> DispatchEvent {
        DispatchEvent::RoomLeave(RoomLeave {
            discord_channel_id: Snowflake::new(1),
            reason: RoomLeaveReason::Left,
        })
    }

    #[tokio::test]
    async fn a_fifth_connection_drops_the_oldest_one() {
        let hub = Hub::new(config());
        let user = Uuid::now_v7();
        let mut receivers = Vec::new();
        let mut sessions = Vec::new();
        for _ in 0..4 {
            let (tx, rx) = mpsc::unbounded_channel();
            receivers.push(rx);
            sessions.push(hub.register(user, tx).await);
        }

        let (tx, _rx) = mpsc::unbounded_channel();
        let fifth = hub.register(user, tx).await;

        // A mais antiga recebeu RECONNECT e saiu do registro.
        let dropped = receivers[0].try_recv().expect("a mais antiga é avisada");
        assert_eq!(dropped, r#"{"op":7}"#);
        assert!(hub.resumable(sessions[0].id, user).await.is_none());
        assert!(hub.resumable(sessions[1].id, user).await.is_some());
        assert!(hub.resumable(fifth.id, user).await.is_some());
    }

    #[tokio::test]
    async fn a_session_id_is_not_a_bearer_token_for_another_user() {
        let hub = Hub::new(config());
        let owner = Uuid::now_v7();
        let attacker = Uuid::now_v7();
        let (tx, _rx) = mpsc::unbounded_channel();
        let session = hub.register(owner, tx).await;

        assert!(hub.resumable(session.id, owner).await.is_some());
        assert!(
            hub.resumable(session.id, attacker).await.is_none(),
            "retomar sessão alheia entregaria o histórico de eventos dela"
        );
    }

    #[tokio::test]
    async fn publishing_reaches_every_session_of_the_user_including_the_origin() {
        let hub = Hub::new(config());
        let user = Uuid::now_v7();
        let (tx_a, mut rx_a) = mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel();
        hub.register(user, tx_a).await;
        hub.register(user, tx_b).await;

        hub.publish_to_user(user, some_event()).await;

        assert!(rx_a.try_recv().is_ok());
        assert!(
            rx_b.try_recv().is_ok(),
            "é isto que mantém duas janelas do mesmo usuário em sincronia"
        );
    }

    #[tokio::test]
    async fn publishing_skips_users_with_no_session() {
        let hub = Hub::new(config());
        let connected = Uuid::now_v7();
        let absent = Uuid::now_v7();
        let (tx, mut rx) = mpsc::unbounded_channel();
        hub.register(connected, tx).await;

        hub.publish_to_users(&[connected, absent], some_event())
            .await;
        assert!(rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn a_disconnected_session_survives_until_the_ttl_passes() {
        let hub = Hub::new(GatewayConfig {
            session_ttl_ms: 0,
            ..config()
        });
        let user = Uuid::now_v7();
        let (tx, _rx) = mpsc::unbounded_channel();
        let session = hub.register(user, tx).await;

        // Ainda conectada: a varredura não a leva.
        assert!(hub.sweep_expired().await.is_empty());
        assert!(hub.resumable(session.id, user).await.is_some());

        session.disconnect();
        let offline = hub.sweep_expired().await;
        assert_eq!(offline, vec![user]);
        assert!(hub.resumable(session.id, user).await.is_none());
    }

    #[tokio::test]
    async fn shutdown_tells_every_session_to_reconnect() {
        let hub = Hub::new(config());
        let (tx, mut rx) = mpsc::unbounded_channel();
        hub.register(Uuid::now_v7(), tx).await;

        hub.broadcast_reconnect().await;
        assert_eq!(rx.try_recv().expect("aviso de reconexão"), r#"{"op":7}"#);
    }
}
