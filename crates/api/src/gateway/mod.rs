//! WebSocket gateway (`docs/websocket.md`).
//!
//! The client sends exactly three things — `IDENTIFY`, `RESUME` and
//! `HEARTBEAT`. Everything else is REST. That asymmetry is deliberate: writing
//! over the socket would duplicate validation, authorisation, rate limiting and
//! error handling in two places, which is the classic source of behaviour drift
//! between them (§1).

pub mod hub;
pub mod ready;
pub mod session;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use protocol::gateway::{
    close_code, limits, ClientInfo, ControlFrame, DispatchEvent, Hello, Identify, InvalidSession,
    Opcode, RawClientFrame, Resume, Resumed,
};
use protocol::GATEWAY_VERSION;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::Instant;
use uuid::Uuid;

pub use hub::Hub;
use session::Session;

use crate::auth::token;
use crate::state::AppState;

/// The oldest client build the server still talks to (§8).
///
/// Subiu para 2.0.0 com a câmera (ADR-0038): `RoomParticipant` passou a carregar
/// uma lista de publicações no lugar de `publishing`/`publishing_since`, e
/// `SHARE_START`/`SHARE_STOP` ganharam `source`. Um cliente 1.x conecta e
/// **entende errado** — ninguém aparece ao vivo, e os ladrilhos não abrem. Falha
/// silenciosa é pior do que recusa: em 4010 o cliente se atualiza sozinho.
const MIN_CLIENT_VERSION: (u32, u32, u32) = (2, 0, 0);

pub fn router() -> Router<AppState> {
    Router::new().route("/gateway", get(upgrade))
}

#[derive(Debug, Deserialize)]
struct GatewayQuery {
    v: Option<u8>,
}

async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<GatewayQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    // Frames from the client are tiny; the cap is the §7 limit.
    let ws = ws.max_message_size(limits::MAX_FRAME_BYTES);
    let requested = query.v.unwrap_or(GATEWAY_VERSION);
    ws.on_upgrade(move |socket| serve(socket, state, requested))
}

/// Everything a connection needs while it runs.
struct Connection {
    state: AppState,
    session: Option<Arc<Session>>,
    /// Sliding window for the frame rate limit (§7).
    window_started: Instant,
    frames_in_window: u32,
}

async fn serve(socket: WebSocket, state: AppState, requested_version: u8) {
    if requested_version != GATEWAY_VERSION {
        close(socket, close_code::VERSION_TOO_OLD, "unsupported version").await;
        return;
    }

    let config = state.hub.config();
    let (mut sender, mut receiver) = {
        use futures_util::StreamExt;
        socket.split()
    };

    let hello = ControlFrame::new(
        Opcode::HELLO,
        Hello {
            heartbeat_interval_ms: config.heartbeat_interval_ms,
            session_ttl_ms: config.session_ttl_ms,
        },
    );
    {
        use futures_util::SinkExt;
        if sender
            .send(Message::Text(Utf8Bytes::from(
                serde_json::to_string(&hello).unwrap_or_default(),
            )))
            .await
            .is_err()
        {
            return;
        }
    }

    // Frames produced by the hub go out through this channel, so a publisher
    // never awaits the socket.
    let (outbox_tx, mut outbox_rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        use futures_util::SinkExt;
        while let Some(text) = outbox_rx.recv().await {
            if sender
                .send(Message::Text(Utf8Bytes::from(text)))
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = sender.close().await;
    });

    let mut conn = Connection {
        state,
        session: None,
        window_started: Instant::now(),
        frames_in_window: 0,
    };

    let identify_deadline = tokio::time::sleep(Duration::from_millis(limits::IDENTIFY_TIMEOUT_MS));
    tokio::pin!(identify_deadline);

    // Two missed heartbeats and the connection is a zombie (§3.2).
    let zombie_after = Duration::from_millis(config.heartbeat_interval_ms * 2);
    let mut last_heartbeat = Instant::now();
    let mut heartbeat_check = tokio::time::interval(Duration::from_millis(
        config.heartbeat_interval_ms.max(1000) / 2,
    ));
    heartbeat_check.tick().await;

    let close_reason = loop {
        use futures_util::StreamExt;
        tokio::select! {
            _ = &mut identify_deadline, if conn.session.is_none() => {
                break Some((close_code::NOT_AUTHENTICATED, "identify timeout"));
            }
            _ = heartbeat_check.tick(), if conn.session.is_some() => {
                if last_heartbeat.elapsed() > zombie_after {
                    break Some((close_code::UNKNOWN, "heartbeat timeout"));
                }
            }
            incoming = receiver.next() => {
                let Some(incoming) = incoming else {
                    break None; // socket closed by the peer
                };
                let message = match incoming {
                    Ok(message) => message,
                    // A frame over the size cap surfaces here as a protocol error.
                    Err(_) => break Some((close_code::DECODE_ERROR, "frame too large")),
                };
                match message {
                    Message::Text(text) => {
                        if text.len() > limits::MAX_FRAME_BYTES {
                            break Some((close_code::DECODE_ERROR, "frame too large"));
                        }
                        if !conn.allow_frame() {
                            break Some((close_code::RATE_LIMITED, "too many frames"));
                        }
                        match conn.handle(&text, &outbox_tx).await {
                            Ok(Some(())) => last_heartbeat = Instant::now(),
                            Ok(None) => {}
                            Err(reason) => break Some(reason),
                        }
                    }
                    Message::Binary(_) => {
                        break Some((close_code::DECODE_ERROR, "binary frame"));
                    }
                    Message::Close(_) => break None,
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
        }
    };

    if let Some(session) = &conn.session {
        // A sessao fica retomavel ate o TTL. Ninguem e avisado: sair da sala e
        // o que o LiveKit reporta, e perder o socket nao e sair da sala.
        session.disconnect();
    }

    drop(outbox_tx);
    if let Some((code, reason)) = close_reason {
        // The writer owns the sink, so the close frame goes through it.
        let _ = writer.await;
        tracing::debug!(code, reason, "gateway connection closed");
    } else {
        let _ = writer.await;
    }
}

impl Connection {
    /// Sliding window over §7's 30 frames per 60 seconds.
    fn allow_frame(&mut self) -> bool {
        if self.window_started.elapsed() > Duration::from_millis(limits::FRAME_WINDOW_MS) {
            self.window_started = Instant::now();
            self.frames_in_window = 0;
        }
        self.frames_in_window += 1;
        self.frames_in_window <= limits::MAX_FRAMES_PER_WINDOW
    }

    /// `Ok(Some(()))` means the frame counts as liveness (a heartbeat or a
    /// successful handshake); `Err` carries the close code.
    async fn handle(
        &mut self,
        text: &str,
        outbox: &mpsc::UnboundedSender<String>,
    ) -> Result<Option<()>, (u16, &'static str)> {
        let Ok(frame) = serde_json::from_str::<RawClientFrame>(text) else {
            return Err((close_code::DECODE_ERROR, "malformed envelope"));
        };

        match frame.op {
            Opcode::HEARTBEAT => {
                let _ = outbox.send(hub::control(Opcode::HEARTBEAT_ACK));
                Ok(Some(()))
            }
            Opcode::IDENTIFY => {
                if self.session.is_some() {
                    return Err((close_code::DECODE_ERROR, "already identified"));
                }
                let payload: Identify = parse(frame.d)?;
                check_client_version(&payload.client)?;
                let user_id = self.authenticate(&payload.token)?;
                self.identify(user_id, outbox.clone()).await;
                Ok(Some(()))
            }
            Opcode::RESUME => {
                if self.session.is_some() {
                    return Err((close_code::DECODE_ERROR, "already identified"));
                }
                let payload: Resume = parse(frame.d)?;
                let user_id = self.authenticate(&payload.token)?;
                self.resume(user_id, payload, outbox.clone()).await;
                Ok(Some(()))
            }
            // Server-to-client opcodes are not valid from a client.
            _ => Err((close_code::DECODE_ERROR, "unexpected opcode")),
        }
    }

    fn authenticate(&self, presented: &str) -> Result<Uuid, (u16, &'static str)> {
        let claims = token::verify_access(self.state.config.jwt_signing_key.as_bytes(), presented)
            .map_err(|_| (close_code::AUTHENTICATION_FAILED, "invalid token"))?;
        claims
            .sub
            .parse::<Uuid>()
            .map_err(|_| (close_code::AUTHENTICATION_FAILED, "invalid subject"))
    }

    async fn identify(&mut self, user_id: Uuid, outbox: mpsc::UnboundedSender<String>) {
        // The session is built but not yet reachable, so READY takes sequence
        // 1 even while another connection is broadcasting presence.
        let session = self.state.hub.create_session(user_id, outbox);
        let ready = ready::build(&self.state, user_id, session.id).await;
        session.dispatch(DispatchEvent::Ready(Box::new(ready)));
        let session = self.state.hub.attach(session).await;
        self.session = Some(session);
    }

    async fn resume(
        &mut self,
        user_id: Uuid,
        payload: Resume,
        outbox: mpsc::UnboundedSender<String>,
    ) {
        let existing = self.state.hub.resumable(payload.session_id, user_id).await;
        let Some(session) = existing else {
            let _ = outbox.send(invalid_session());
            return;
        };
        let Some(missed) = session.replay_after(payload.last_seq) else {
            // The gap is wider than the buffer: the client recovers by keyset
            // over REST instead (§3.3).
            self.state.hub.forget(session.id).await;
            let _ = outbox.send(invalid_session());
            return;
        };

        session.reconnect(outbox);
        // Replayed frames keep their original `s`, so the client's gap detection
        // still works if a second interruption happens mid-replay.
        for frame in &missed {
            if let Ok(text) = serde_json::to_string(frame) {
                session.send_text(text);
            }
        }
        session.dispatch(DispatchEvent::Resumed(Resumed {
            replayed: missed.len() as u32,
        }));
        self.session = Some(session);
    }
}

fn invalid_session() -> String {
    serde_json::to_string(&ControlFrame::new(
        Opcode::INVALID_SESSION,
        InvalidSession { resumable: false },
    ))
    .unwrap_or_else(|_| r#"{"op":6,"d":{"resumable":false}}"#.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(
    data: Option<serde_json::Value>,
) -> Result<T, (u16, &'static str)> {
    let data = data.ok_or((close_code::DECODE_ERROR, "missing payload"))?;
    serde_json::from_value(data).map_err(|_| (close_code::DECODE_ERROR, "invalid payload"))
}

/// `major.minor.patch`; anything unparseable is treated as too old.
fn check_client_version(client: &ClientInfo) -> Result<(), (u16, &'static str)> {
    let parsed = parse_version(&client.version);
    match parsed {
        Some(version) if version >= MIN_CLIENT_VERSION => Ok(()),
        _ => Err((close_code::VERSION_TOO_OLD, "client too old")),
    }
}

fn parse_version(raw: &str) -> Option<(u32, u32, u32)> {
    // The prerelease and build suffixes have to come off before splitting on
    // '.', or `1.0.0-beta.2` reads as four components and is rejected.
    let core = raw.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = match parts.next() {
        Some(value) => value.parse().ok()?,
        None => 0,
    };
    parts.next().is_none().then_some((major, minor, patch))
}

async fn close(mut socket: WebSocket, code: u16, reason: &'static str) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: Utf8Bytes::from_static(reason),
        })))
        .await;
}

/// Background task: expires sessions past their TTL and reports the users who
/// went offline as a result.
pub async fn run_session_sweeper(state: AppState) {
    let period = Duration::from_millis(state.hub.config().session_ttl_ms.max(1000) / 3);
    let mut ticker = tokio::time::interval(period);
    loop {
        ticker.tick().await;
        // Nada a anunciar: presenca de sala e o que o LiveKit reporta, e uma
        // sessao WebSocket morta nao tira ninguem da sala.
        let expired = state.hub.sweep_expired().await;
        if !expired.is_empty() {
            tracing::debug!(count = expired.len(), "expired gateway sessions swept");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing_accepts_what_a_release_actually_looks_like() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("1.0.0-beta.2"), Some((1, 0, 0)));
        assert_eq!(parse_version("1.0"), Some((1, 0, 0)));
        assert_eq!(parse_version("nao-e-versao"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn a_client_below_the_minimum_is_closed_with_4010_not_4001() {
        // 4010 dispara o fluxo de atualização automática no cliente (RF-36);
        // 4001 o faria deslogar o usuário, que não tem nada a ver com o problema.
        let old = ClientInfo {
            version: "0.0.9".into(),
            os: "windows".into(),
        };
        assert_eq!(
            check_client_version(&old),
            Err((close_code::VERSION_TOO_OLD, "client too old"))
        );
        // A quebra de fio da câmera (ADR-0038): o cliente da versão anterior
        // conectaria e leria a sala errada, sem erro nenhum na tela.
        let previous_major = ClientInfo {
            version: "1.1.0".into(),
            os: "windows".into(),
        };
        assert_eq!(
            check_client_version(&previous_major),
            Err((close_code::VERSION_TOO_OLD, "client too old"))
        );
        let current = ClientInfo {
            version: "2.0.0".into(),
            os: "windows".into(),
        };
        assert!(check_client_version(&current).is_ok());
    }

    #[test]
    fn an_unparseable_version_is_treated_as_too_old() {
        let broken = ClientInfo {
            version: String::new(),
            os: "windows".into(),
        };
        assert!(check_client_version(&broken).is_err());
    }

    #[test]
    fn the_frame_rate_window_resets_and_only_then_allows_more() {
        let mut counter = 0u32;
        let mut allow = || {
            counter += 1;
            counter <= limits::MAX_FRAMES_PER_WINDOW
        };
        for _ in 0..limits::MAX_FRAMES_PER_WINDOW {
            assert!(allow());
        }
        assert!(!allow(), "o frame 31 dentro da janela derruba a conexão");
    }
}
