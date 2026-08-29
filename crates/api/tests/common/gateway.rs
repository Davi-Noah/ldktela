//! A real HTTP server on a real port with a real WebSocket client.
//!
//! Resume, heartbeats and disconnection cannot be exercised through
//! `oneshot`: they are properties of a live socket. These tests bind
//! `127.0.0.1:0` and talk to it with `tokio-tungstenite`.

#![allow(dead_code)]

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use super::TestApp;

pub struct RunningApp {
    pub app: TestApp,
    pub addr: std::net::SocketAddr,
    _server: tokio::task::JoinHandle<()>,
}

impl RunningApp {
    pub async fn spawn() -> Self {
        let app = TestApp::spawn().await;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binding an ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        let router = app.router.clone();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Self {
            app,
            addr,
            _server: server,
        }
    }

    pub async fn connect(&self) -> Client {
        let url = format!("ws://{}/gateway?v=1", self.addr);
        let (socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("websocket handshake");
        Client { socket }
    }
}

pub struct Client {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

impl Client {
    /// Reads the next JSON frame, failing the test rather than hanging forever.
    pub async fn recv(&mut self) -> Value {
        self.try_recv()
            .await
            .expect("esperava um frame e o socket ficou em silêncio")
    }

    pub async fn try_recv(&mut self) -> Option<Value> {
        let deadline = Duration::from_secs(5);
        loop {
            match tokio::time::timeout(deadline, self.socket.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    return Some(serde_json::from_str(&text).expect("frame is JSON"))
                }
                Ok(Some(Ok(Message::Close(_)))) | Ok(None) => return None,
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(_))) => return None,
                Err(_) => return None,
            }
        }
    }

    /// Reads until an event with this name arrives, or gives up.
    pub async fn recv_event(&mut self, name: &str) -> Value {
        for _ in 0..40 {
            let Some(frame) = self.try_recv().await else {
                panic!("socket fechou antes de {name}");
            };
            if frame["t"] == name {
                return frame;
            }
        }
        panic!("{name} não chegou em 40 frames");
    }

    /// Collects every frame that arrives within `millis`.
    pub async fn drain(&mut self, millis: u64) -> Vec<Value> {
        let mut frames = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(millis);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, self.socket.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    if let Ok(value) = serde_json::from_str(&text) {
                        frames.push(value);
                    }
                }
                Ok(Some(Ok(_))) => {}
                _ => break,
            }
        }
        frames
    }

    pub async fn send(&mut self, frame: Value) {
        self.socket
            .send(Message::Text(frame.to_string().into()))
            .await
            .expect("sending frame");
    }

    pub async fn identify(&mut self, token: &str) -> Value {
        let hello = self.recv().await;
        assert_eq!(hello["op"], 1, "o primeiro frame precisa ser HELLO");
        self.send(json!({
            "op": 2,
            "d": { "token": token, "client": { "version": "0.1.0", "os": "windows" } }
        }))
        .await;
        let ready = self.recv().await;
        assert_eq!(ready["t"], "READY", "esperava READY, veio {ready}");
        ready
    }

    pub async fn resume(&mut self, token: &str, session_id: Uuid, last_seq: u64) -> Value {
        let hello = self.recv().await;
        assert_eq!(hello["op"], 1);
        self.send(json!({
            "op": 3,
            "d": { "token": token, "session_id": session_id, "last_seq": last_seq }
        }))
        .await;
        self.recv().await
    }

    /// Drops the socket without a close handshake, the way a network blip does.
    pub async fn kill(self) {
        drop(self.socket);
    }

    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}
