//! LiveKit integration: room tokens, the publisher guard and webhook
//! verification (RF-13 to RF-17, RNF-05, RNF-06).
//!
//! The backend never touches media. It does three things:
//!
//! * issues a room-scoped JWT after Discord has said the caller may be there;
//! * refuses a publisher beyond the ceiling, because egress is the only scarce
//!   resource the product has (RNF-05);
//! * receives LiveKit's webhooks and turns them into `SHARE_START`/`SHARE_STOP`,
//!   which is what makes a screen share visible to everyone else in the room.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use livekit_api::access_token::{AccessToken, TokenVerifier, VideoGrants};
use livekit_api::services::room::RoomClient;
use livekit_api::webhooks::WebhookReceiver;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{AppError, UpstreamError};

#[derive(Debug, Clone)]
pub struct RoomConfig {
    /// WebSocket URL the client connects to.
    pub url: String,
    pub api_key: String,
    pub api_secret: String,
    /// RNF-06 caps this at 60 minutes.
    pub token_ttl_seconds: u64,
    /// Simultaneous publishers per room (P-01, default 2).
    pub max_publishers: usize,
}

/// The hard ceiling from RNF-06. A configuration above it is clamped rather
/// than refused: a long-lived media token is a security property, not a taste.
pub const MAX_TOKEN_TTL_SECONDS: u64 = 3600;

/// LiveKit track sources this product ever publishes (ADR-0012). A viewer
/// publishes nothing at all, and nobody ever publishes a camera or a microphone.
const PUBLISHABLE_SOURCES: [&str; 2] = ["screen_share", "screen_share_audio"];

/// Suffix that tells the publishing connection apart from the watching one.
///
/// A person who shares is in the room twice: the WebView watching, and the core
/// publishing (ADR-0027). LiveKit disconnects the first participant when a
/// second arrives with the same identity, so the two need different names — and
/// the name is stamped into the signed token here, never chosen by the client,
/// or a client could take someone else's.
///
/// `~` was picked because it cannot occur in a UUID, which makes stripping it
/// unambiguous.
pub const PUBLISHER_SUFFIX: &str = "~pub";

/// The identity the core uses to publish for `user_id`.
pub fn publisher_identity(user_id: Uuid) -> String {
    format!("{user_id}{PUBLISHER_SUFFIX}")
}

/// Room name for a Discord voice channel. Deterministic, so two clients whose
/// Discord put them in the same channel land in the same room with no
/// coordination between them (ADR-0011).
pub fn room_name(discord_channel_id: i64) -> String {
    format!("dvc-{discord_channel_id}")
}

/// The Discord voice channel a room name refers to, or `None` if it is not ours.
pub fn channel_of_room(room: &str) -> Option<i64> {
    room.strip_prefix("dvc-")?.parse().ok()
}

/// The HTTP origin of the SFU, derived from the WebSocket URL the clients use.
///
/// One URL in configuration instead of two: they always point at the same
/// server, and two variables is two chances to point them somewhere different.
fn http_origin(ws_url: &str) -> String {
    match ws_url.split_once("://") {
        Some(("ws", rest)) => format!("http://{rest}"),
        Some(("wss", rest)) => format!("https://{rest}"),
        _ => ws_url.to_owned(),
    }
}

pub struct Rooms {
    config: RoomConfig,
    receiver: WebhookReceiver,
    /// Server-side room control. Used for exactly one thing: throwing someone
    /// out the moment Discord says they may no longer be there (RF-08).
    client: RoomClient,
    /// Who currently holds a publish grant, per Discord voice channel.
    ///
    /// This is admission control at token issue. LiveKit stays the authority on
    /// what is actually published, but by the time a third screen is live the
    /// egress is already spent. The process is a single instance (RNF-14), so
    /// the ledger lives in memory.
    publisher_grants: RwLock<HashMap<i64, HashSet<Uuid>>>,
}

impl Rooms {
    pub fn new(config: RoomConfig) -> Self {
        let receiver = WebhookReceiver::new(TokenVerifier::with_api_key(
            &config.api_key,
            &config.api_secret,
        ));
        let client = RoomClient::with_api_key(
            &http_origin(&config.url),
            &config.api_key,
            &config.api_secret,
        );
        Self {
            config,
            receiver,
            client,
            publisher_grants: RwLock::new(HashMap::new()),
        }
    }

    /// Throws a participant out of a room, now.
    ///
    /// This is the enforcement half of RF-08. Waiting for the token to expire
    /// would leave someone watching a screen they lost the right to see for up
    /// to an hour, and a screen share session routinely lasts longer than that.
    /// Both of them: someone who is sharing is in the room twice, and throwing
    /// out only the viewer would leave their screen on everyone else's display
    /// (ADR-0027).
    ///
    /// The publishing identity is usually absent — most people are not sharing —
    /// so its removal failing is expected and only logged. The viewer's is not:
    /// that one is the revocation, and it has to be reported if it fails.
    pub async fn remove_participant(
        &self,
        discord_channel_id: i64,
        user_id: Uuid,
    ) -> Result<(), AppError> {
        let room = room_name(discord_channel_id);

        if let Err(e) = self
            .client
            .remove_participant(&room, &publisher_identity(user_id))
            .await
        {
            tracing::debug!(error = %e, %user_id, "no publishing connection to remove");
        }

        self.client
            .remove_participant(&room, &user_id.to_string())
            .await
            .map(|_| ())
            .map_err(|e| {
                AppError::Upstream(UpstreamError::LiveKit(format!("removing participant: {e}")))
            })
    }

    pub fn url(&self) -> &str {
        &self.config.url
    }

    pub fn token_ttl_seconds(&self) -> i64 {
        self.config.token_ttl_seconds.min(MAX_TOKEN_TTL_SECONDS) as i64
    }

    pub fn max_publishers(&self) -> usize {
        self.config.max_publishers
    }

    /// Reserves a publish slot, or reports the room is full.
    ///
    /// Re-requesting a publish token while already holding a slot is not a new
    /// publisher: the client renews silently before expiry (RNF-06), and
    /// counting the renewal would lock a user out of their own screen share.
    pub async fn claim_publisher(
        &self,
        discord_channel_id: i64,
        user_id: Uuid,
    ) -> Result<(), AppError> {
        let mut grants = self.publisher_grants.write().await;
        let room = grants.entry(discord_channel_id).or_default();
        if room.contains(&user_id) {
            return Ok(());
        }
        if room.len() >= self.config.max_publishers {
            return Err(AppError::RoomCapacity);
        }
        room.insert(user_id);
        Ok(())
    }

    /// Gives back a slot: the user stopped sharing, left, or asked for a viewer
    /// token.
    pub async fn release_publisher(&self, discord_channel_id: i64, user_id: Uuid) {
        let mut grants = self.publisher_grants.write().await;
        if let Some(room) = grants.get_mut(&discord_channel_id) {
            room.remove(&user_id);
            if room.is_empty() {
                grants.remove(&discord_channel_id);
            }
        }
    }

    pub async fn publisher_slots_taken(&self, discord_channel_id: i64) -> usize {
        self.publisher_grants
            .read()
            .await
            .get(&discord_channel_id)
            .map_or(0, HashSet::len)
    }

    /// A room-scoped token.
    ///
    /// The grant names exactly one room and nothing else (RNF-06): the token
    /// cannot be replayed against another channel and carries no administrative
    /// capability. A viewer's token cannot publish anything at all — the
    /// topology is one-way by construction, not by client good behaviour.
    pub fn issue_token(
        &self,
        discord_channel_id: i64,
        user_id: Uuid,
        display_name: &str,
        publish: bool,
    ) -> Result<String, AppError> {
        let grants = VideoGrants {
            room_join: true,
            room: room_name(discord_channel_id),
            can_subscribe: true,
            can_publish: publish,
            can_publish_data: false,
            can_publish_sources: if publish {
                PUBLISHABLE_SOURCES
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect()
            } else {
                Vec::new()
            },
            ..Default::default()
        };

        let identity = if publish {
            publisher_identity(user_id)
        } else {
            user_id.to_string()
        };

        AccessToken::with_api_key(&self.config.api_key, &self.config.api_secret)
            .with_identity(&identity)
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
    /// endpoint is the **only** source of `SHARE_START`, so accepting one would
    /// let anyone announce anyone as sharing in any room.
    pub fn verify_webhook(&self, body: &str, authorization: &str) -> Option<WebhookEvent> {
        match self.receiver.receive(body, authorization) {
            Ok(event) => Some(WebhookEvent::from_proto(event)),
            Err(err) => {
                tracing::warn!(error = %err, "rejected livekit webhook");
                None
            }
        }
    }

    /// Drops the publisher ledger for a room LiveKit has closed.
    ///
    /// Closing the room itself is LiveKit's job: `empty_timeout` and
    /// `departure_timeout` expire an unoccupied room. A sweeper here would be a
    /// second, weaker implementation of a lifecycle the SFU already owns.
    pub async fn forget_room(&self, discord_channel_id: i64) {
        self.publisher_grants
            .write()
            .await
            .remove(&discord_channel_id);
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

    /// The Discord voice channel this event belongs to, if it is one of ours.
    pub fn channel(&self) -> Option<i64> {
        self.room.as_deref().and_then(channel_of_room)
    }

    /// The local user id carried in the participant identity.
    ///
    /// Both connections of one person answer with the same id: the suffix is a
    /// transport detail, and everything downstream — presence, sessions, the
    /// publisher ledger — is keyed by the person.
    pub fn user(&self) -> Option<Uuid> {
        let identity = self.participant_identity.as_deref()?;
        identity
            .strip_suffix(PUBLISHER_SUFFIX)
            .unwrap_or(identity)
            .parse()
            .ok()
    }

    /// Whether the event came from the publishing connection rather than the
    /// person.
    ///
    /// It decides whether a join or a leave touches presence. A publishing
    /// connection appearing is not someone entering the room, and it going away
    /// is not someone leaving — it is a screen starting and stopping.
    pub fn is_publisher_connection(&self) -> bool {
        self.participant_identity
            .as_deref()
            .is_some_and(|id| id.ends_with(PUBLISHER_SUFFIX))
    }

    /// Whether this event is about the screen video track.
    ///
    /// Compared by equality, not by prefix: sharing a screen with audio
    /// publishes **two** tracks, `screen_share` and `screen_share_audio`. With a
    /// `contains` check, unpublishing only the audio would clear the sharing
    /// state of someone whose screen is still on everyone's display.
    pub fn is_screen_video(&self) -> bool {
        self.track_source.as_deref() == Some("screen_share")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANNEL: i64 = 1_234_567_890_123_456_789;

    fn config() -> RoomConfig {
        RoomConfig {
            url: "ws://localhost:7880".into(),
            api_key: "devkey".into(),
            api_secret: "dev-only-not-a-real-key-0123456789abcdef".into(),
            token_ttl_seconds: 3600,
            max_publishers: 2,
        }
    }

    #[test]
    fn a_room_name_round_trips_to_its_channel() {
        assert_eq!(channel_of_room(&room_name(CHANNEL)), Some(CHANNEL));
        assert_eq!(channel_of_room("outra-coisa"), None);
        assert_eq!(channel_of_room("dvc-nao-e-numero"), None);
    }

    #[test]
    fn a_snowflake_beyond_2_pow_53_survives_the_round_trip() {
        // O nome da sala passa por JSON no webhook do LiveKit; um id truncado
        // colocaria o evento na sala errada.
        assert_eq!(
            channel_of_room(&room_name(9_007_199_254_740_993)),
            Some(9_007_199_254_740_993)
        );
    }

    #[test]
    fn the_token_ttl_is_clamped_to_the_ceiling() {
        let rooms = Rooms::new(RoomConfig {
            token_ttl_seconds: 86_400,
            ..config()
        });
        assert_eq!(rooms.token_ttl_seconds(), MAX_TOKEN_TTL_SECONDS as i64);
    }

    /// The grant block of a token, decoded. The JWT body is base64url with no
    /// padding, between the dots.
    fn grants(token: &str) -> serde_json::Value {
        let payload = token.split('.').nth(1).expect("payload");
        let decoded = base64_decode(payload);
        let claims: serde_json::Value = serde_json::from_str(&decoded).expect("payload é JSON");
        claims["video"].clone()
    }

    #[test]
    fn a_viewer_token_cannot_publish_anything() {
        let rooms = Rooms::new(config());
        let token = rooms
            .issue_token(CHANNEL, Uuid::nil(), "pessoa", false)
            .expect("emitindo token de espectador");
        let video = grants(&token);

        assert_eq!(video["canPublish"], false);
        assert_eq!(video["canPublishData"], false);
        assert_eq!(video["canSubscribe"], true);
        assert!(
            video["canPublishSources"]
                .as_array()
                .is_none_or(|s| s.is_empty()),
            "espectador nao publica fonte nenhuma: {video}"
        );
    }

    #[test]
    fn a_publisher_token_is_limited_to_the_screen() {
        let rooms = Rooms::new(config());
        let token = rooms
            .issue_token(CHANNEL, Uuid::nil(), "pessoa", true)
            .expect("emitindo token de publicador");
        let video = grants(&token);

        assert_eq!(video["canPublish"], true);
        let sources: Vec<&str> = video["canPublishSources"]
            .as_array()
            .expect("lista de fontes")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(sources, vec!["screen_share", "screen_share_audio"]);
        assert!(
            !sources.contains(&"camera") && !sources.contains(&"microphone"),
            "a topologia e unidirecional por construcao (ADR-0012): {video}"
        );
    }

    #[test]
    fn a_token_is_scoped_to_one_room_and_carries_no_admin_rights() {
        let rooms = Rooms::new(config());
        let token = rooms
            .issue_token(CHANNEL, Uuid::nil(), "pessoa", true)
            .expect("emitindo token");
        let video = grants(&token);

        assert_eq!(video["room"], room_name(CHANNEL));
        assert_eq!(video["roomJoin"], true);
        // Presentes como `false` no payload; o que nao pode e serem `true`.
        for capability in ["roomAdmin", "roomCreate", "roomList", "roomRecord"] {
            assert_eq!(
                video[capability], false,
                "{capability} nao pode vir habilitado: {video}"
            );
        }
    }

    fn base64_decode(input: &str) -> String {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(input)
            .expect("payload base64url");
        String::from_utf8(bytes).expect("payload utf-8")
    }

    #[tokio::test]
    async fn the_publisher_guard_refuses_beyond_the_ceiling() {
        let rooms = Rooms::new(config());
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let c = Uuid::now_v7();

        rooms.claim_publisher(CHANNEL, a).await.expect("primeiro");
        rooms.claim_publisher(CHANNEL, b).await.expect("segundo");
        assert!(
            rooms.claim_publisher(CHANNEL, c).await.is_err(),
            "o terceiro publicador deveria ser recusado"
        );
        assert_eq!(rooms.publisher_slots_taken(CHANNEL).await, 2);
    }

    #[tokio::test]
    async fn renewing_does_not_consume_a_second_slot() {
        let rooms = Rooms::new(config());
        let a = Uuid::now_v7();
        rooms.claim_publisher(CHANNEL, a).await.expect("primeiro");
        rooms.claim_publisher(CHANNEL, a).await.expect("renovacao");
        assert_eq!(rooms.publisher_slots_taken(CHANNEL).await, 1);
    }

    #[tokio::test]
    async fn releasing_frees_the_slot_for_someone_else() {
        let rooms = Rooms::new(config());
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let c = Uuid::now_v7();
        rooms.claim_publisher(CHANNEL, a).await.expect("a");
        rooms.claim_publisher(CHANNEL, b).await.expect("b");
        rooms.release_publisher(CHANNEL, a).await;
        rooms.claim_publisher(CHANNEL, c).await.expect("c entra");
        assert_eq!(rooms.publisher_slots_taken(CHANNEL).await, 2);
    }

    #[tokio::test]
    async fn slots_are_per_room() {
        let rooms = Rooms::new(config());
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        rooms.claim_publisher(CHANNEL, a).await.expect("a");
        rooms.claim_publisher(CHANNEL, b).await.expect("b");
        rooms.claim_publisher(999, a).await.expect("outra sala");
        assert_eq!(rooms.publisher_slots_taken(999).await, 1);
    }

    #[tokio::test]
    async fn forgetting_a_room_clears_its_ledger() {
        let rooms = Rooms::new(config());
        rooms
            .claim_publisher(CHANNEL, Uuid::now_v7())
            .await
            .expect("claim");
        rooms.forget_room(CHANNEL).await;
        assert_eq!(rooms.publisher_slots_taken(CHANNEL).await, 0);
    }

    #[test]
    fn an_unsigned_webhook_is_refused() {
        let rooms = Rooms::new(config());
        assert!(rooms.verify_webhook("{}", "").is_none());
        assert!(rooms.verify_webhook("{}", "Bearer nao-e-um-jwt").is_none());
    }

    /// Uma pessoa que compartilha esta na sala duas vezes (ADR-0027), e o resto
    /// do sistema — presenca, sessoes, o ledger de publicadores — e indexado
    /// pela pessoa. Se o sufixo vazasse, cada metade viraria um usuario.
    #[test]
    fn both_connections_of_one_person_resolve_to_the_same_user() {
        let user = Uuid::now_v7();
        let viewer = WebhookEvent {
            event: "participant_joined".into(),
            room: Some(room_name(CHANNEL)),
            participant_identity: Some(user.to_string()),
            track_source: None,
        };
        let publisher = WebhookEvent {
            participant_identity: Some(publisher_identity(user)),
            ..viewer.clone()
        };

        assert_eq!(viewer.user(), Some(user));
        assert_eq!(publisher.user(), Some(user));
        assert!(!viewer.is_publisher_connection());
        assert!(publisher.is_publisher_connection());
    }

    #[test]
    fn a_publish_token_carries_the_suffixed_identity_and_a_viewer_token_does_not() {
        // Quem decide a identidade e o servidor, dentro do JWT assinado: se o
        // cliente escolhesse, poderia assumir a de outra pessoa.
        let rooms = Rooms::new(config());
        let user = Uuid::now_v7();

        let publish = rooms
            .issue_token(CHANNEL, user, "pessoa", true)
            .expect("token de publicacao");
        let view = rooms
            .issue_token(CHANNEL, user, "pessoa", false)
            .expect("token de espectador");

        assert_eq!(identity_of(&publish), publisher_identity(user));
        assert_eq!(identity_of(&view), user.to_string());
    }

    /// The `sub` claim of a token, which is what LiveKit uses as the identity.
    fn identity_of(token: &str) -> String {
        let payload = token.split('.').nth(1).expect("payload");
        let claims: serde_json::Value =
            serde_json::from_str(&base64_decode(payload)).expect("payload é JSON");
        claims["sub"].as_str().unwrap_or_default().to_owned()
    }

    #[test]
    fn an_identity_that_is_not_ours_is_not_mistaken_for_a_publisher() {
        // Um agente ou uma ferramenta de inspecao pode entrar na sala; nada
        // disso pode virar presenca nem sessao de tela.
        let stranger = WebhookEvent {
            event: "participant_joined".into(),
            room: Some(room_name(CHANNEL)),
            participant_identity: Some("gravador-do-suporte".into()),
            track_source: None,
        };
        assert_eq!(stranger.user(), None);
        assert!(!stranger.is_publisher_connection());
    }

    #[test]
    fn screen_audio_is_not_mistaken_for_screen_video() {
        let video = WebhookEvent {
            event: "track_published".into(),
            room: Some(room_name(CHANNEL)),
            participant_identity: None,
            track_source: Some("screen_share".into()),
        };
        let audio = WebhookEvent {
            track_source: Some("screen_share_audio".into()),
            ..video.clone()
        };
        assert!(video.is_screen_video());
        assert!(
            !audio.is_screen_video(),
            "despublicar so o audio nao pode derrubar o estado da tela"
        );
    }

    /// Guarda metade do par de versoes do LiveKit (ADR-0019); a outra metade, o
    /// `livekit-client`, e verificada em `desktop/src/media/versions.test.ts`.
    ///
    /// Uma tag movel como `v1.8` troca a versao do SFU embaixo do cliente sem
    /// ninguem decidir. Quando as duas pontas divergem, so a publicacao quebra:
    /// a negociacao expira em 15 s e o cliente reconecta em laco, enquanto
    /// parear, entrar na sala e assinar seguem funcionando e o servidor responde
    /// 200 em tudo.
    #[test]
    fn the_dev_compose_pins_an_exact_livekit_server_version() {
        let compose = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docker/compose.dev.yml"
        ))
        .expect("docker/compose.dev.yml deve existir");

        let image = compose
            .lines()
            .filter_map(|line| line.trim().strip_prefix("image:"))
            .map(str::trim)
            .find(|image| image.starts_with("livekit/livekit-server:"))
            .expect("o compose deve declarar a imagem do livekit-server");

        let tag = image
            .split_once(':')
            .map(|(_, tag)| tag)
            .unwrap_or_default();
        let exact = tag.strip_prefix('v').is_some_and(|version| {
            let parts: Vec<&str> = version.split('.').collect();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        });

        assert!(
            exact,
            "a imagem esta como {image}. Uma tag movel muda o SFU sem ninguem \
             decidir; fixe vX.Y.Z e releia o ADR-0019 antes de trocar."
        );
    }
}
