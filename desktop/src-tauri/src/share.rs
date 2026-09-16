//! The commands the interface calls to share a screen (RF-37, ADR-0026).
//!
//! The core is a dumb media engine on purpose: it is handed a URL, a token, a
//! source and a preset, and it publishes. It does not know this product has a
//! REST API, and it never authenticates anything. The TypeScript side already
//! holds the session and asks the server for the publish token, so putting a
//! second copy of that here would be a second place for it to go wrong.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

use crate::capture::{self, Capture, ShareSource, SourceKind};
use crate::publisher::{Preset, Publisher};

/// Emitted when a share ends without the user asking: the SFU dropped us, or the
/// window being shared was closed. The interface has to notice, because the
/// button still says "stop sharing".
const ENDED_EVENT: &str = "share://ended";

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    pub url: String,
    pub token: String,
    pub source_id: String,
    pub kind: SourceKind,
    pub preset: Preset,
    pub audio: bool,
}

#[derive(Debug, Serialize)]
pub struct StartedShare {
    /// `null` when sharing without audio; otherwise which of the two capture
    /// modes we actually got, so the interface can warn (RF-30).
    pub audio: Option<AudioModeReport>,
}

/// Mirrors `audio::AudioMode`, but exists on every platform so the wire shape
/// does not change with the build target.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioModeReport {
    ExcludingDiscord,
    WholeSystem,
}

#[derive(Debug, thiserror::Error)]
pub enum ShareFailure {
    #[error("ja existe um compartilhamento em andamento")]
    AlreadySharing,
    #[error("identificador de fonte invalido")]
    BadSource,
    #[error(transparent)]
    Capture(#[from] crate::capture::CaptureError),
    #[error(transparent)]
    Publish(#[from] crate::publisher::PublishError),
}

/// Unlike the vault's error, this one is meant to be read: every variant is a
/// message this project wrote, and the user has no way to act on "falhou" alone.
impl Serialize for ShareFailure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Default)]
pub struct Sharing {
    active: Mutex<Option<Active>>,
}

struct Active {
    publisher: Publisher,
    capture: Capture,
    #[cfg(target_os = "windows")]
    audio: Option<crate::audio::AudioCapture>,
}

/// What can be shared right now.
///
/// Our own windows are filtered out by handle rather than by title: two windows
/// can share a title, and a user who happens to name something "ldkcord" should
/// still be able to share it.
#[tauri::command]
pub fn share_sources(app: AppHandle) -> Vec<ShareSource> {
    capture::list_sources(&own_windows(&app))
}

#[tauri::command]
pub async fn share_start(
    app: AppHandle,
    state: State<'_, Sharing>,
    request: StartRequest,
) -> Result<StartedShare, ShareFailure> {
    let mut active = state.active.lock().await;
    if active.is_some() {
        return Err(ShareFailure::AlreadySharing);
    }

    let source_id: u64 = request
        .source_id
        .parse()
        .map_err(|_| ShareFailure::BadSource)?;

    let ended = app.clone();
    let publisher = Publisher::start(
        &request.url,
        &request.token,
        request.preset,
        request.audio,
        move |reason| {
            eprintln!("compartilhamento: o servidor de midia encerrou ({reason})");
            let _ = ended.emit(ENDED_EVENT, reason);
        },
    )
    .await?;

    let lost = app.clone();
    let capture = match capture::start(
        request.kind,
        source_id,
        request.preset.ceiling(),
        request.preset.fps(),
        publisher.video_sink(),
        move || {
            let _ = lost.emit(ENDED_EVENT, "fonte encerrada");
        },
    ) {
        Ok(capture) => capture,
        Err(error) => {
            // A sala ja esta aberta: sair dela e o que impede um publicador
            // fantasma de segurar a vaga de admissao ate o token expirar.
            publisher.stop().await;
            return Err(error.into());
        }
    };

    let audio = start_audio(&publisher);

    *active = Some(Active {
        publisher,
        capture,
        #[cfg(target_os = "windows")]
        audio: audio.0,
    });
    Ok(StartedShare { audio: audio.1 })
}

#[tauri::command]
pub async fn share_stop(state: State<'_, Sharing>) -> Result<(), ShareFailure> {
    let taken = state.active.lock().await.take();
    if let Some(active) = taken {
        // A captura para antes da sala fechar: o contrario deixaria quadros
        // sendo empurrados para uma fonte que o encoder ja largou.
        active.capture.stop();
        #[cfg(target_os = "windows")]
        if let Some(audio) = active.audio {
            audio.stop();
        }
        active.publisher.stop().await;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn start_audio(
    publisher: &Publisher,
) -> (Option<crate::audio::AudioCapture>, Option<AudioModeReport>) {
    let Some(sink) = publisher.audio_sink() else {
        return (None, None);
    };
    match crate::audio::start(sink) {
        Ok((capture, mode)) => {
            let report = match mode {
                crate::audio::AudioMode::ExcludingDiscord => AudioModeReport::ExcludingDiscord,
                crate::audio::AudioMode::WholeSystem => AudioModeReport::WholeSystem,
            };
            (Some(capture), Some(report))
        }
        Err(error) => {
            // A tela continua no ar sem som. Perder o compartilhamento inteiro
            // porque o audio falhou seria pior do que compartilhar em silencio.
            eprintln!("compartilhamento: sem audio ({error})");
            (None, None)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn start_audio(_publisher: &Publisher) -> ((), Option<AudioModeReport>) {
    ((), None)
}

/// The handles of our own top-level windows.
///
/// libwebrtc's window source id **is** the `HWND` on Windows, which is what
/// makes this comparison exact instead of a guess at the title.
#[cfg(target_os = "windows")]
fn own_windows(app: &AppHandle) -> Vec<u64> {
    app.webview_windows()
        .values()
        .filter_map(|window| window.hwnd().ok())
        .map(|hwnd| hwnd.0 as usize as u64)
        .collect()
}

#[cfg(not(target_os = "windows"))]
fn own_windows(_app: &AppHandle) -> Vec<u64> {
    Vec::new()
}
