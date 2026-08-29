//! LiveKit integration: room tokens, the camera guard and webhook verification
//! (RF-19 to RF-23, RNF-07, RNF-10).
//!
//! The backend never touches media. It does three things:
//!
//! * issues a room-scoped JWT after checking `CONNECT_VOICE`;
//! * refuses a fourth camera publisher, because egress is the scarce resource
//!   (RNF-10);
//! * receives LiveKit's webhooks and turns them into `VOICE_STATE_UPDATE`, which
//!   is what makes voice state visible to people who are not in the room
//!   (RF-20).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use livekit_api::access_token::{AccessToken, TokenVerifier, VideoGrants};
use livekit_api::webhooks::WebhookReceiver;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{AppError, UpstreamError};

#[derive(Debug, Clone)]
pub struct VoiceConfig {
    /// WebSocket URL the client connects to.
    pub url: String,
    pub api_key: String,
    pub api_secret: String,
    /// RNF-07 caps this at 60 minutes.
    pub token_ttl_seconds: u64,
    /// RNF-10: camera publishers per room.
    pub max_camera_publishers: usize,
    /// RNF-10: rooms with no audio for this long are closed.
    pub idle_room_timeout_seconds: u64,
}

/// The hard ceiling from RNF-07. A configuration above it is clamped rather
/// than refused: a long-lived media token is a security property, not a taste.
pub const MAX_TOKEN_TTL_SECONDS: u64 = 3600;

/// Room name for a channel. Deterministic, so two clients joining the same
/// channel land in the same room without any coordination.
pub fn room_name(channel_id: Uuid) -> String {
    format!("channel-{channel_id}")
}

/// The channel a room name refers to, or `None` if it is not one of ours.
pub fn channel_of_room(room: &str) -> Option<Uuid> {
    room.strip_prefix("channel-")?.parse().ok()
}

pub struct Voice {
    config: VoiceConfig,
    receiver: WebhookReceiver,
    /// Who currently holds a camera grant, per channel.
    ///
    /// The grant is admission control at token issue: LiveKit is the authority
    /// on what is actually published, but by the time a fourth camera is live
    /// the egress is already spent. The schema has no column for camera intent
    /// (SRS §5.2 has `streaming` for screen share only), and the process is a
    /// single instance (RNF-17), so the ledger lives in memory.
    camera_grants: RwLock<HashMap<Uuid, HashSet<Uuid>>>,
    /// Last time a room was seen carrying audio, for the idle sweep.
    audio_seen: RwLock<HashMap<Uuid, tokio::time::Instant>>,
}

impl Voice {
    pub fn new(config: VoiceConfig) -> Self {
        let receiver = WebhookReceiver::new(TokenVerifier::with_api_key(
            &config.api_key,
            &config.api_secret,
        ));
        Self {
            config,
            receiver,
            camera_grants: RwLock::new(HashMap::new()),
            audio_seen: RwLock::new(HashMap::new()),
        }
    }

    pub fn url(&self) -> &str {
        &self.config.url
    }

    pub fn token_ttl_seconds(&self) -> i64 {
        self.config.token_ttl_seconds.min(MAX_TOKEN_TTL_SECONDS) as i64
    }

    /// Reserves a camera slot, or reports the room is full.
    ///
    /// Re-requesting a token with camera while already holding a slot is not a
    /// new publisher: a client renews silently before expiry (RNF-07), and
    /// counting the renewal would lock the user out of their own camera.
    pub async fn claim_camera(&self, channel_id: Uuid, user_id: Uuid) -> Result<(), AppError> {
        let mut grants = self.camera_grants.write().await;
        let room = grants.entry(channel_id).or_default();
        if room.contains(&user_id) {
            return Ok(());
        }
        if room.len() >= self.config.max_camera_publishers {
            return Err(AppError::VoiceCapacity);
        }
        room.insert(user_id);
        Ok(())
    }

    /// Gives back a camera slot: the user left, or asked for a listener token.
    pub async fn release_camera(&self, channel_id: Uuid, user_id: Uuid) {
        let mut grants = self.camera_grants.write().await;
        if let Some(room) = grants.get_mut(&channel_id) {
            room.remove(&user_id);
            if room.is_empty() {
                grants.remove(&channel_id);
            }
        }
    }

    pub async fn camera_publishers(&self, channel_id: Uuid) -> usize {
        self.camera_grants
            .read()
            .await
            .get(&channel_id)
            .map_or(0, HashSet::len)
    }

    /// A room-scoped token.
    ///
    /// The grant names exactly one room and nothing else (RNF-07): with
    /// `room_join` and a room name, the token cannot be replayed against another
    /// channel, and it carries no administrative capability.
    pub fn issue_token(
        &self,
        channel_id: Uuid,
        user_id: Uuid,
        display_name: &str,
        publish_camera: bool,
    ) -> Result<String, AppError> {
        let mut grants = VideoGrants {
            room_join: true,
            room: room_name(channel_id),
            can_subscribe: true,
            can_publish: true,
            can_publish_data: true,
            ..Default::default()
        };
        if !publish_camera {
            // Screen share and microphone stay available; only the camera is
            // withheld, so a listener can still speak and present.
            grants.can_publish_sources = vec![
                "microphone".to_string(),
                "screen_share".to_string(),
                "screen_share_audio".to_string(),
            ];
        }

        AccessToken::with_api_key(&self.config.api_key, &self.config.api_secret)
            .with_identity(&user_id.to_string())
            .with_name(display_name)
            .with_ttl(Duration::from_secs(
                self.config.token_ttl_seconds.min(MAX_TOKEN_TTL_SECONDS),
            ))
            .with_grants(grants)
            .to_jwt()
            .map_err(|e| AppError::Upstream(UpstreamError::LiveKit(format!("issuing token: {e}"))))
    }

    /// Verifies a webhook body against its `Authorization` header.
    ///
    /// An unsigned or mis-signed body is refused, not merely logged: this
    /// endpoint is the **only** source of `VOICE_STATE_UPDATE` (§6.9), so
    /// accepting one would let anyone place anyone in any voice channel.
    pub fn verify_webhook(&self, body: &str, authorization: &str) -> Option<WebhookEvent> {
        match self.receiver.receive(body, authorization) {
            Ok(event) => Some(WebhookEvent::from_proto(event)),
            Err(err) => {
                tracing::warn!(error = %err, "rejected livekit webhook");
                None
            }
        }
    }

    /// Records that a room is carrying audio right now.
    pub async fn mark_audio(&self, channel_id: Uuid) {
        self.audio_seen
            .write()
            .await
            .insert(channel_id, tokio::time::Instant::now());
    }

    pub async fn forget_room(&self, channel_id: Uuid) {
        self.audio_seen.write().await.remove(&channel_id);
        self.camera_grants.write().await.remove(&channel_id);
    }

    /// Rooms that have gone quiet past the RNF-10 timeout.
    pub async fn idle_rooms(&self) -> Vec<Uuid> {
        let timeout = Duration::from_secs(self.config.idle_room_timeout_seconds);
        let now = tokio::time::Instant::now();
        self.audio_seen
            .read()
            .await
            .iter()
            .filter(|(_, seen)| now.duration_since(**seen) >= timeout)
            .map(|(channel_id, _)| *channel_id)
            .collect()
    }
}

/// The parts of a LiveKit webhook this application acts on.
#[derive(Debug, Clone)]
pub struct WebhookEvent {
    pub event: String,
    pub room: Option<String>,
    pub participant_identity: Option<String>,
    pub track_source: Option<String>,
}

impl WebhookEvent {
    fn from_proto(event: livekit_protocol::WebhookEvent) -> Self {
        let track_source = event
            .track
            .as_ref()
            .map(|t| t.source().as_str_name().to_ascii_lowercase());
        Self {
            event: event.event,
            room: event.room.map(|r| r.name),
            participant_identity: event.participant.map(|p| p.identity),
            track_source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> VoiceConfig {
        VoiceConfig {
            url: "ws://localhost:7880".into(),
            api_key: "devkey".into(),
            api_secret: "dev-only-not-a-real-key-0123456789abcdef".into(),
            token_ttl_seconds: 3600,
            max_camera_publishers: 3,
            idle_room_timeout_seconds: 900,
        }
    }

    #[test]
    fn a_room_name_round_trips_to_its_channel() {
        let channel = Uuid::now_v7();
        assert_eq!(channel_of_room(&room_name(channel)), Some(channel));
        assert_eq!(channel_of_room("outra-coisa"), None);
        assert_eq!(channel_of_room("channel-nao-e-uuid"), None);
    }

    #[test]
    fn the_token_ttl_is_clamped_to_the_rnf07_ceiling() {
        let voice = Voice::new(VoiceConfig {
            token_ttl_seconds: 86_400,
            ..config()
        });
        assert_eq!(
            voice.token_ttl_seconds(),
            3600,
            "um token de mídia de longa duração é uma propriedade de segurança"
        );
    }

    #[test]
    fn the_token_is_scoped_to_one_room_and_carries_no_admin_grant() {
        let voice = Voice::new(config());
        let channel = Uuid::now_v7();
        let user = Uuid::now_v7();
        let jwt = voice.issue_token(channel, user, "Fulano", true).unwrap();

        let claims = livekit_api::access_token::Claims::from_unverified(&jwt).unwrap();
        assert_eq!(claims.video.room, room_name(channel));
        assert!(claims.video.room_join);
        assert!(!claims.video.room_admin, "sem capacidade administrativa");
        assert!(!claims.video.room_create);
        assert_eq!(claims.sub, user.to_string());
    }

    #[test]
    fn a_listener_token_withholds_the_camera_but_keeps_voice_and_screen() {
        let voice = Voice::new(config());
        let jwt = voice
            .issue_token(Uuid::now_v7(), Uuid::now_v7(), "Fulano", false)
            .unwrap();
        let claims = livekit_api::access_token::Claims::from_unverified(&jwt).unwrap();
        let sources = &claims.video.can_publish_sources;
        assert!(sources.iter().any(|s| s == "microphone"));
        assert!(sources.iter().any(|s| s == "screen_share"));
        assert!(
            !sources.iter().any(|s| s == "camera"),
            "sem slot de câmera, a fonte não pode estar no token: {sources:?}"
        );
    }

    #[tokio::test]
    async fn the_fourth_camera_is_refused_and_a_release_frees_the_slot() {
        let voice = Voice::new(config());
        let channel = Uuid::now_v7();
        let users: Vec<Uuid> = (0..4).map(|_| Uuid::now_v7()).collect();

        for user in &users[..3] {
            voice.claim_camera(channel, *user).await.unwrap();
        }
        assert_eq!(voice.camera_publishers(channel).await, 3);
        assert!(matches!(
            voice.claim_camera(channel, users[3]).await,
            Err(AppError::VoiceCapacity)
        ));

        voice.release_camera(channel, users[0]).await;
        voice
            .claim_camera(channel, users[3])
            .await
            .expect("a vaga liberada é reutilizável");
    }

    #[tokio::test]
    async fn renewing_a_token_does_not_consume_a_second_slot() {
        // O cliente renova silenciosamente antes de expirar (RNF-07); contar a
        // renovação trancaria o usuário fora da própria câmera.
        let voice = Voice::new(config());
        let channel = Uuid::now_v7();
        let user = Uuid::now_v7();
        for _ in 0..5 {
            voice.claim_camera(channel, user).await.unwrap();
        }
        assert_eq!(voice.camera_publishers(channel).await, 1);
    }

    #[tokio::test]
    async fn the_camera_ledger_is_per_room() {
        let voice = Voice::new(config());
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        for _ in 0..3 {
            voice.claim_camera(a, Uuid::now_v7()).await.unwrap();
        }
        assert!(voice.claim_camera(a, Uuid::now_v7()).await.is_err());
        voice
            .claim_camera(b, Uuid::now_v7())
            .await
            .expect("outra sala tem o próprio orçamento");
    }

    #[test]
    fn an_unsigned_body_is_refused() {
        let voice = Voice::new(config());
        assert!(voice.verify_webhook("{}", "").is_none());
        assert!(voice.verify_webhook("{}", "nao-e-um-jwt").is_none());
        // Um corpo válido com assinatura de outra chave também não passa.
        let forged = AccessToken::with_api_key("devkey", "outro-segredo-completamente")
            .with_sha256("Zm9v")
            .to_jwt()
            .unwrap();
        assert!(voice.verify_webhook("{}", &forged).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_room_goes_idle_only_after_the_configured_timeout() {
        let voice = Voice::new(config());
        let channel = Uuid::now_v7();
        voice.mark_audio(channel).await;
        assert!(voice.idle_rooms().await.is_empty());

        tokio::time::advance(Duration::from_secs(899)).await;
        assert!(voice.idle_rooms().await.is_empty());

        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(voice.idle_rooms().await, vec![channel]);

        voice.forget_room(channel).await;
        assert!(voice.idle_rooms().await.is_empty());
    }
}
