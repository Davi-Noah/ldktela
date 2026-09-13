//! One gateway session: its sequence counter, its resume buffer and the channel
//! that carries frames to whichever connection currently owns it.
//!
//! A session outlives its connection. When the socket drops, the session stays
//! resumable for `session_ttl_ms` (`docs/protocol/websocket.md` §3.3), which is
//! what lets a client reconnect through a network blip without reloading state.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use protocol::gateway::{DispatchEvent, DispatchFrame};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Frames queued for the socket. Unbounded because a slow reader is handled by
/// dropping the connection, not by blocking the publisher.
pub type Outbox = mpsc::UnboundedSender<String>;

pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    /// Monotonic per session, starting at 1 (§2).
    seq: AtomicU64,
    /// The last `resume_buffer_size` replayable dispatches, oldest first.
    buffer: Mutex<VecDeque<DispatchFrame>>,
    buffer_capacity: usize,
    /// Highest sequence dropped from the buffer because it overflowed.
    ///
    /// A gap is unreplayable only when something replayable was *evicted*.
    /// Comparing against the oldest buffered sequence instead would call a
    /// skipped ephemeral event a gap: `TYPING_START` consumes a sequence and
    /// is deliberately never buffered (websocket.md 5).
    evicted_through: AtomicU64,
    /// `None` while the session is disconnected but still resumable.
    outbox: Mutex<Option<Outbox>>,
    connected: AtomicBool,
    /// When the socket dropped, for the TTL sweep.
    disconnected_at: Mutex<Option<OffsetDateTime>>,
}

impl Session {
    pub fn new(user_id: Uuid, outbox: Outbox, buffer_capacity: usize) -> Self {
        Self {
            id: Uuid::now_v7(),
            user_id,
            seq: AtomicU64::new(0),
            buffer: Mutex::new(VecDeque::with_capacity(buffer_capacity.min(64))),
            buffer_capacity,
            evicted_through: AtomicU64::new(0),
            outbox: Mutex::new(Some(outbox)),
            connected: AtomicBool::new(true),
            disconnected_at: Mutex::new(None),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// Marks the socket gone. The session stays resumable until the TTL sweep.
    pub fn disconnect(&self) {
        self.connected.store(false, Ordering::Release);
        *self.outbox.lock().expect("outbox mutex") = None;
        *self.disconnected_at.lock().expect("disconnect mutex") = Some(OffsetDateTime::now_utc());
    }

    /// Attaches a new socket to an existing session (`RESUME`).
    pub fn reconnect(&self, outbox: Outbox) {
        *self.outbox.lock().expect("outbox mutex") = Some(outbox);
        *self.disconnected_at.lock().expect("disconnect mutex") = None;
        self.connected.store(true, Ordering::Release);
    }

    pub fn disconnected_at(&self) -> Option<OffsetDateTime> {
        *self.disconnected_at.lock().expect("disconnect mutex")
    }

    pub fn last_seq(&self) -> u64 {
        self.seq.load(Ordering::Acquire)
    }

    /// Assigns the next sequence number, records the frame for replay and sends
    /// it if a socket is attached.
    ///
    /// The sequence advances even while disconnected: that is what makes a gap
    /// detectable on resume.
    pub fn dispatch(&self, event: DispatchEvent) {
        let seq = self.seq.fetch_add(1, Ordering::AcqRel) + 1;
        let frame = DispatchFrame::new(seq, event);

        {
            // Todo evento entra no buffer. v1 tinha um efemero (TYPING_START)
            // que consumia sequencia sem ser bufferizado, e era por causa dele
            // que a lacuna precisava ser medida por eviccao em vez de pelo
            // frame mais antigo. A medicao por eviccao continua correta; o caso
            // especial e que sumiu.
            let mut buffer = self.buffer.lock().expect("buffer mutex");
            if buffer.len() == self.buffer_capacity {
                if let Some(dropped) = buffer.pop_front() {
                    self.evicted_through.store(dropped.s, Ordering::Release);
                }
            }
            buffer.push_back(frame.clone());
        }
        self.send_frame(&frame);
    }

    fn send_frame(&self, frame: &DispatchFrame) {
        let Ok(text) = serde_json::to_string(frame) else {
            tracing::error!(event = frame.event.name(), "dispatch failed to serialise");
            return;
        };
        self.send_text(text);
    }

    /// Sends a pre-serialised frame (control frames carry no sequence).
    pub fn send_text(&self, text: String) {
        let outbox = self.outbox.lock().expect("outbox mutex");
        if let Some(tx) = outbox.as_ref() {
            // A closed receiver means the socket is already gone; the TTL sweep
            // will collect the session.
            let _ = tx.send(text);
        }
    }

    /// Frames after `last_seq`, in order, or `None` when the gap is wider than
    /// the buffer and the client has to recover over REST instead.
    pub fn replay_after(&self, last_seq: u64) -> Option<Vec<DispatchFrame>> {
        let current = self.seq.load(Ordering::Acquire);
        if last_seq > current {
            // The client claims to have seen more than was ever sent.
            return None;
        }
        // Anything the buffer had to drop is gone for good.
        if last_seq < self.evicted_through.load(Ordering::Acquire) {
            return None;
        }
        let buffer = self.buffer.lock().expect("buffer mutex");
        Some(buffer.iter().filter(|f| f.s > last_seq).cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::gateway::Resumed;
    use protocol::room::{RoomLeave, RoomLeaveReason};
    use protocol::scalars::Snowflake;

    fn session(capacity: usize) -> (Session, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Session::new(Uuid::now_v7(), tx, capacity), rx)
    }

    fn event() -> DispatchEvent {
        DispatchEvent::RoomLeave(RoomLeave {
            discord_channel_id: Snowflake::new(1),
            reason: RoomLeaveReason::Left,
        })
    }

    #[test]
    fn sequence_starts_at_one_and_never_repeats() {
        let (session, mut rx) = session(10);
        session.dispatch(event());
        session.dispatch(event());
        let first: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        let second: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(first["s"], 1);
        assert_eq!(second["s"], 2);
        assert_eq!(session.last_seq(), 2);
    }

    #[test]
    fn the_buffer_drops_the_oldest_frame_when_full() {
        let (session, _rx) = session(3);
        for _ in 0..5 {
            session.dispatch(event());
        }
        // Só os três últimos continuam retomáveis.
        assert!(
            session.replay_after(1).is_none(),
            "lacuna maior que o buffer"
        );
        let replay = session.replay_after(2).expect("dentro do buffer");
        assert_eq!(replay.len(), 3);
        assert_eq!(replay[0].s, 3);
        assert_eq!(replay[2].s, 5);
    }

    #[test]
    fn every_event_is_replayable() {
        // v1 tinha um evento efemero (TYPING_START) que consumia sequencia sem
        // entrar no buffer. Sem ele, sequencia e buffer andam juntos, e este
        // teste falha se alguem reintroduzir a excecao sem pensar no resume.
        let (session, _rx) = session(10);
        session.dispatch(event());
        session.dispatch(event());

        assert_eq!(session.last_seq(), 2);
        let replay = session.replay_after(0).expect("replay");
        assert_eq!(replay.len(), 2);
    }

    #[test]
    fn a_client_claiming_more_than_was_sent_cannot_resume() {
        let (session, _rx) = session(10);
        session.dispatch(event());
        assert!(session.replay_after(99).is_none());
    }

    #[test]
    fn nothing_missed_replays_as_an_empty_list_not_as_a_failure() {
        let (session, _rx) = session(10);
        session.dispatch(event());
        let replay = session.replay_after(1).expect("sem lacuna");
        assert!(replay.is_empty());
    }

    #[test]
    fn the_sequence_advances_while_disconnected_so_the_gap_is_detectable() {
        let (session, mut rx) = session(10);
        session.dispatch(event());
        rx.try_recv().unwrap();

        session.disconnect();
        assert!(!session.is_connected());
        session.dispatch(event());
        session.dispatch(DispatchEvent::Resumed(Resumed { replayed: 0 }));
        assert_eq!(session.last_seq(), 3);

        let (tx, mut rx2) = mpsc::unbounded_channel();
        session.reconnect(tx);
        assert!(session.is_connected());
        let replay = session.replay_after(1).expect("dentro do buffer");
        assert_eq!(replay.len(), 2, "os dois frames perdidos precisam voltar");
        assert!(
            rx2.try_recv().is_err(),
            "o replay é explícito, não automático"
        );
    }
}
