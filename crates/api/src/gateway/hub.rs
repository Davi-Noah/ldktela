//! The session registry and the publish surface the REST routes call.
//!
//! Every mutation in `crates/api/src/routes` that changes state the client can
//! see ends in a `Hub::publish_*` call; `docs/api/rest-api.md` §6 makes that a
//! contract, not a nicety.

use std::collections::HashMap;
use std::sync::Arc;

use db::PgPool;
use protocol::gateway::{ControlFrame, DispatchEvent, Opcode, PermissionsStale};
use protocol::user::{Presence, PresenceStatus};
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::GatewayConfig;
use crate::gateway::index::ViewerIndex;
use crate::gateway::session::{Outbox, Session};

#[derive(Default)]
struct Registry {
    sessions: HashMap<Uuid, Arc<Session>>,
    /// Session ids per user, oldest first, so the 4-connection limit can drop
    /// the oldest (`docs/protocol/websocket.md` §7).
    by_user: HashMap<Uuid, Vec<Uuid>>,
}

pub struct Hub {
    config: GatewayConfig,
    registry: RwLock<Registry>,
    index: ViewerIndex,
    /// Status the user set explicitly. `online`/`offline` derive from whether a
    /// session is connected (`docs/api/rest-api.md` §6.1).
    declared: RwLock<HashMap<Uuid, PresenceStatus>>,
}

impl Hub {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            registry: RwLock::new(Registry::default()),
            index: ViewerIndex::new(),
            declared: RwLock::new(HashMap::new()),
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
    /// contract (websocket.md 3.1), and building it takes several queries.
    /// Registering first leaves a window in which another connection's
    /// presence broadcast claims sequence 1.
    pub fn create_session(&self, user_id: Uuid, outbox: Outbox) -> Arc<Session> {
        Arc::new(Session::new(
            user_id,
            outbox,
            self.config.resume_buffer_size,
        ))
    }

    /// Publishes a session, dropping the user's oldest one if they are
    /// already at the connection limit.
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

    /// What third parties may see. `invisible` is reported as `offline` (RF-04).
    pub async fn presence_of(&self, user_id: Uuid, viewer: Uuid) -> PresenceStatus {
        let declared = self.declared.read().await.get(&user_id).copied();
        let status = match declared {
            Some(status) => status,
            None if self.is_online(user_id).await => PresenceStatus::Online,
            None => PresenceStatus::Offline,
        };
        // A declared status only survives while the user is connected.
        let effective = if status != PresenceStatus::Offline && !self.is_online(user_id).await {
            PresenceStatus::Offline
        } else {
            status
        };
        if user_id == viewer {
            effective
        } else {
            effective.visible_to_others()
        }
    }

    pub async fn declare_presence(&self, user_id: Uuid, status: PresenceStatus) {
        if status == PresenceStatus::Online {
            self.declared.write().await.remove(&user_id);
        } else {
            self.declared.write().await.insert(user_id, status);
        }
    }

    // -----------------------------------------------------------------------
    // Publicação
    // -----------------------------------------------------------------------

    /// Sends to every session of every user in `users`, including the session
    /// that caused the change (§4.3).
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

    /// Who can currently see the channel. Callers that are about to delete the
    /// channel capture this first, because after the row is gone there is
    /// nobody left to compute.
    pub async fn viewers(&self, pool: &PgPool, channel_id: Uuid) -> Vec<Uuid> {
        self.index
            .viewers(pool, channel_id)
            .await
            .into_iter()
            .collect()
    }

    /// Sends to whoever can currently see the channel (§4.1).
    pub async fn publish_to_channel(&self, pool: &PgPool, channel_id: Uuid, event: DispatchEvent) {
        let viewers: Vec<Uuid> = self
            .index
            .viewers(pool, channel_id)
            .await
            .into_iter()
            .collect();
        self.publish_to_users(&viewers, event).await;
    }

    /// Sends to the guild's members. Used for events that belong to no channel:
    /// presence, membership and roles.
    pub async fn publish_to_guild(&self, pool: &PgPool, guild_id: Uuid, event: DispatchEvent) {
        let members = db::repo::permissions::guild_member_ids(pool, guild_id)
            .await
            .unwrap_or_default();
        self.publish_to_users(&members, event).await;
    }

    /// Sends only to the user's own sessions (`READ_STATE_UPDATE`).
    pub async fn publish_to_user(&self, user_id: Uuid, event: DispatchEvent) {
        self.publish_to_users(&[user_id], event).await;
    }

    // -----------------------------------------------------------------------
    // Invalidação (os cinco gatilhos de §4.2)
    // -----------------------------------------------------------------------

    /// A channel overwrite changed, or the channel was created or removed.
    pub async fn invalidate_channel(
        &self,
        pool: &PgPool,
        channel_id: Uuid,
        guild_id: Option<Uuid>,
    ) {
        self.index.invalidate_channel(channel_id).await;
        if let Some(guild_id) = guild_id {
            self.announce_stale(pool, guild_id).await;
        }
    }

    /// A role, a role assignment or the membership set changed. Every channel of
    /// the guild can have moved, so the whole guild's entries go.
    pub async fn invalidate_guild(&self, pool: &PgPool, guild_id: Uuid) {
        self.index.invalidate_guild(guild_id).await;
        self.announce_stale(pool, guild_id).await;
    }

    /// Tells the guild its permission view may be wrong. Without this a demoted
    /// user keeps seeing controls the server will refuse (§5).
    async fn announce_stale(&self, pool: &PgPool, guild_id: Uuid) {
        self.publish_to_guild(
            pool,
            guild_id,
            DispatchEvent::PermissionsStale(PermissionsStale { guild_id }),
        )
        .await;
    }

    /// Graceful shutdown: every session is told to reconnect, and the TTL gives
    /// the process time to come back (§3.4).
    pub async fn broadcast_reconnect(&self) {
        let registry = self.registry.read().await;
        for session in registry.sessions.values() {
            session.send_text(control(Opcode::RECONNECT));
        }
    }

    /// Presence of everyone the viewer shares a guild with.
    pub async fn presences_for(&self, pool: &PgPool, viewer: Uuid) -> Vec<Presence> {
        let guilds = db::repo::guilds::list_for_user(pool, viewer)
            .await
            .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for guild in guilds {
            let members = db::repo::permissions::guild_member_ids(pool, guild.id)
                .await
                .unwrap_or_default();
            for member in members {
                if seen.insert(member) {
                    out.push(Presence {
                        user_id: member,
                        status: self.presence_of(member, viewer).await,
                    });
                }
            }
        }
        out
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
    use tokio::sync::mpsc;

    fn config() -> GatewayConfig {
        GatewayConfig {
            heartbeat_interval_ms: 30_000,
            session_ttl_ms: 90_000,
            resume_buffer_size: 500,
            max_connections_per_user: 4,
        }
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

        hub.publish_to_user(
            user,
            DispatchEvent::PermissionsStale(PermissionsStale {
                guild_id: Uuid::nil(),
            }),
        )
        .await;

        assert!(rx_a.try_recv().is_ok());
        assert!(
            rx_b.try_recv().is_ok(),
            "é isto que sincroniza leitura e presença entre máquinas"
        );
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
    async fn invisible_is_offline_to_others_and_invisible_to_oneself() {
        let hub = Hub::new(config());
        let user = Uuid::now_v7();
        let other = Uuid::now_v7();
        let (tx, _rx) = mpsc::unbounded_channel();
        hub.register(user, tx).await;

        hub.declare_presence(user, PresenceStatus::Invisible).await;
        assert_eq!(
            hub.presence_of(user, other).await,
            PresenceStatus::Offline,
            "PRESENCE_UPDATE nunca revela invisible"
        );
        assert_eq!(hub.presence_of(user, user).await, PresenceStatus::Invisible);

        hub.declare_presence(user, PresenceStatus::Dnd).await;
        assert_eq!(hub.presence_of(user, other).await, PresenceStatus::Dnd);

        hub.declare_presence(user, PresenceStatus::Online).await;
        assert_eq!(hub.presence_of(user, other).await, PresenceStatus::Online);
    }

    #[tokio::test]
    async fn a_declared_status_does_not_outlive_the_connection() {
        let hub = Hub::new(config());
        let user = Uuid::now_v7();
        let (tx, _rx) = mpsc::unbounded_channel();
        let session = hub.register(user, tx).await;
        hub.declare_presence(user, PresenceStatus::Dnd).await;
        assert_eq!(hub.presence_of(user, user).await, PresenceStatus::Dnd);

        session.disconnect();
        assert_eq!(
            hub.presence_of(user, user).await,
            PresenceStatus::Offline,
            "sem batimento não há presença, qualquer que seja o status declarado"
        );
    }
}
