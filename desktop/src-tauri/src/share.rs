//! The commands the interface calls to share a screen (RF-37, ADR-0026).
//!
//! The core is a dumb media engine on purpose: it is handed a URL, a token, a
//! source and a preset, and it publishes. It does not know this product has a
//! REST API, and it never authenticates anything. The TypeScript side already
//! holds the session and asks the server for the publish token, so putting a
//! second copy of that here would be a second place for it to go wrong.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

use crate::capture::{self, Capture, ShareSource, SourceKind};
use crate::preview::{self, Preview, GRID_FPS, THUMBNAIL_MAX};
use crate::publisher::{Preset, Publisher, PublisherStats, Source};

/// Emitted when a publication ends without the user asking: the SFU dropped us,
/// the window being shared was closed, or the camera was unplugged. The
/// interface has to notice, because the button still says "stop".
///
/// Carries the source since ADR-0038: parar a camera nao pode apagar o botao da
/// tela, que continua no ar.
const ENDED_EVENT: &str = "share://ended";

/// Why one publication ended, and which one.
#[derive(Debug, Clone, Serialize)]
pub struct ShareEnded {
    pub source: Source,
    pub reason: String,
}

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
    OnlyWindow,
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
    #[error("ja existe uma camera no ar")]
    AlreadyOnCamera,
    #[cfg(target_os = "windows")]
    #[error(transparent)]
    Camera(#[from] crate::camera::CameraError),
    #[cfg(not(target_os = "windows"))]
    #[error("camera so no Windows por enquanto")]
    NoCameraHere,
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
    live: Mutex<Live>,
    /// Uma miniatura por vez.
    ///
    /// O seletor pede todas de uma vez, e cada pedido abre um `DesktopCapturer`
    /// proprio: com quinze janelas abertas, quinze capturadores DXGI subindo no
    /// mesmo instante. Enfileirar custa menos de um segundo no total e os
    /// cartoes preenchem conforme chegam.
    thumbnails: Mutex<()>,
}

/// Uma conexao de publicacao, ate duas publicacoes em cima dela (ADR-0038).
///
/// O `Publisher` vive enquanto **qualquer** fonte estiver no ar: a camera entra
/// por cima da sessao que a tela abriu, e vice-versa. Reconectar para ligar a
/// segunda derrubaria a primeira, que e exatamente o que o ADR-0027 evita.
#[derive(Default)]
struct Live {
    publisher: Option<Arc<Publisher>>,
    screen: Option<ScreenActive>,
    camera: Option<CameraActive>,
}

impl Live {
    /// Fecha a conexao quando nada mais esta no ar.
    ///
    /// Sair da sala e o que devolve a vaga de admissao no servidor; deixar a
    /// conexao aberta sem trilha nenhuma seguraria a vaga ate o token expirar.
    async fn close_if_idle(&mut self) {
        if self.screen.is_some() || self.camera.is_some() {
            return;
        }
        // Fecha pelo `Arc`, e nao tomando posse: quem chama daqui costuma ainda
        // segurar um clone — os caminhos de erro de `share_start` e de
        // `camera_start` seguram. Exigir posse exclusiva deixaria uma conexao
        // conectada e sem trilha nenhuma, que e um publicador fantasma
        // ocupando a vaga de admissao ate o token expirar.
        if let Some(publisher) = self.publisher.take() {
            publisher.stop().await;
        }
    }
}

struct ScreenActive {
    capture: Capture,
    preview: Preview,
    /// Guardado a parte do `Preview` para que ligar e desligar o preview nao
    /// precise tomar posse de nada.
    preview_control: Arc<preview::Control>,
    #[cfg(target_os = "windows")]
    audio: Option<crate::audio::AudioCapture>,
}

#[cfg(target_os = "windows")]
struct CameraActive {
    capture: crate::camera::CameraCapture,
    preview: Preview,
    preview_control: Arc<preview::Control>,
}

/// Sem camera fora do Windows: o Media Foundation e o caminho da plataforma, e
/// nao ha segundo alvo hoje (ADR-0038).
#[cfg(not(target_os = "windows"))]
struct CameraActive {
    preview: Preview,
    preview_control: Arc<preview::Control>,
}

/// What can be shared right now.
///
/// Our own windows are filtered out by handle rather than by title: two windows
/// can share a title, and a user who happens to name something "ldktela" should
/// still be able to share it.
#[tauri::command]
pub fn share_sources(app: AppHandle) -> Vec<ShareSource> {
    capture::list_sources(&own_windows(&app))
}

/// Uma miniatura de uma fonte, como data URL (RF-37).
///
/// Pedida uma por uma, e nao junto da lista, de proposito: capturar quinze
/// janelas custa quase um segundo, e o seletor precisa abrir na hora. A lista
/// aparece imediatamente e as figuras entram conforme chegam.
///
/// Devolve `None` quando a fonte nao entrega quadro — janela minimizada, ou
/// fechada entre listar e capturar. O seletor cai no cartao sem imagem, que
/// continua utilizavel.
#[tauri::command]
pub async fn share_thumbnail(
    state: State<'_, Sharing>,
    kind: SourceKind,
    source_id: String,
) -> Result<Option<String>, ShareFailure> {
    let Ok(id) = source_id.parse::<u64>() else {
        return Ok(None);
    };
    let _queued = state.thumbnails.lock().await;
    let frame =
        tauri::async_runtime::spawn_blocking(move || capture::thumbnail(kind, id, THUMBNAIL_MAX))
            .await
            .ok()
            .flatten();
    Ok(frame.as_ref().and_then(preview::encode_data_url))
}

/// Liga, desliga e acelera o preview da propria tela (ADR-0030).
///
/// `fps` sobe quando o preview esta em foco e cai quando ele volta a ser um
/// ladrilho da grade. Desligado, o ramo para na thread de captura — nao e o
/// elemento que some, e a subamostragem que deixa de acontecer.
#[tauri::command]
pub async fn share_preview(
    state: State<'_, Sharing>,
    enabled: bool,
    fps: u32,
    focused: bool,
) -> Result<(), ShareFailure> {
    let live = state.live.lock().await;
    if let Some(screen) = live.screen.as_ref() {
        screen.preview_control.set(enabled, fps, focused);
    }
    Ok(())
}

#[tauri::command]
pub async fn share_start(
    app: AppHandle,
    state: State<'_, Sharing>,
    request: StartRequest,
) -> Result<StartedShare, ShareFailure> {
    let mut live = state.live.lock().await;
    if live.screen.is_some() {
        return Err(ShareFailure::AlreadySharing);
    }

    let source_id: u64 = request
        .source_id
        .parse()
        .map_err(|_| ShareFailure::BadSource)?;

    let publisher = connect(&mut live, &app, &request.url, &request.token).await?;
    let (video, audio_sink) = match publisher
        .publish_screen(request.preset, request.audio)
        .await
    {
        Ok(sinks) => sinks,
        Err(error) => {
            live.close_if_idle().await;
            return Err(error.into());
        }
    };

    let (preview, tap) = preview::start(app.clone(), Source::Screen);
    let preview_control = preview.control();
    preview_control.set(true, GRID_FPS, false);

    let lost = app.clone();
    let capture = match capture::start(
        request.kind,
        source_id,
        request.preset.ceiling(),
        request.preset.fps(),
        video,
        Some(tap),
        move || give_up(&lost, Source::Screen, "fonte encerrada".to_owned()),
    ) {
        Ok(capture) => capture,
        Err(error) => {
            preview.stop();
            publisher.unpublish(Source::Screen).await;
            live.close_if_idle().await;
            return Err(error.into());
        }
    };

    let audio = start_audio(audio_sink, request.kind, source_id);

    live.screen = Some(ScreenActive {
        capture,
        preview,
        preview_control,
        #[cfg(target_os = "windows")]
        audio: audio.0,
    });
    Ok(StartedShare { audio: audio.1 })
}

/// Desfaz, no core, uma publicacao que morreu sozinha.
///
/// Avisar a interface nao basta, e foi exatamente esse o defeito: o React
/// apagava o botao e o Rust continuava achando que a fonte estava no ar. A
/// proxima tentativa batia em "ja existe uma camera no ar", a trilha seguia
/// publicada, e a sala inteira ficava olhando um ladrilho preto com o cronometro
/// correndo, porque o servidor nunca recebeu o `SHARE_STOP`.
///
/// Chamada **de dentro da thread de captura**, entao ela agenda e volta na hora.
/// Bloquear aqui seria pedir para a tarefa agendada dar `join` nesta mesma
/// thread enquanto ela espera.
fn give_up(app: &AppHandle, source: Source, reason: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(state) = app.try_state::<Sharing>() {
            let mut live = state.live.lock().await;
            release(&mut live, source).await;
        }
        let _ = app.emit(ENDED_EVENT, ShareEnded { source, reason });
    });
}

/// Para a captura e despublica uma fonte, se ela ainda estiver de pe.
async fn release(live: &mut Live, source: Source) {
    match source {
        Source::Screen => {
            if let Some(screen) = live.screen.take() {
                screen.capture.stop();
                screen.preview.stop();
                #[cfg(target_os = "windows")]
                if let Some(audio) = screen.audio {
                    audio.stop();
                }
            }
        }
        Source::Camera => {
            if let Some(camera) = live.camera.take() {
                #[cfg(target_os = "windows")]
                camera.capture.stop();
                camera.preview.stop();
            }
        }
    }
    if let Some(publisher) = live.publisher.as_ref() {
        publisher.unpublish(source).await;
    }
    live.close_if_idle().await;
}

/// A conexao de publicacao, reaproveitada quando ja existe (ADR-0038).
async fn connect(
    live: &mut Live,
    app: &AppHandle,
    url: &str,
    token: &str,
) -> Result<Arc<Publisher>, ShareFailure> {
    if let Some(publisher) = live.publisher.as_ref() {
        return Ok(Arc::clone(publisher));
    }
    let ended = app.clone();
    let publisher = Arc::new(
        Publisher::connect(url, token, move |reason| {
            eprintln!("compartilhamento: o servidor de midia encerrou ({reason})");
            // A conexao caiu inteira, entao as duas fontes cairam com ela — e as
            // duas precisam ser desfeitas aqui, ou ligar de novo esbarra num
            // compartilhamento que so existe na nossa cabeca.
            for source in [Source::Screen, Source::Camera] {
                // Dito como "conexao", e nao so o motivo cru do LiveKit: no
                // aviso que a pessoa le, isto precisa ser distinguivel de uma
                // camera que o Windows derrubou. Sao subsistemas diferentes, e
                // a mesma frase para os dois nao deixa ninguem investigar nada.
                give_up(
                    &ended,
                    source,
                    format!("a conexao de transmissao caiu ({reason})"),
                );
            }
        })
        .await?,
    );
    live.publisher = Some(Arc::clone(&publisher));
    Ok(publisher)
}

/// Sampled by the interface on a timer. `None` when nothing is being shared.
#[tauri::command]
pub async fn share_stats(
    state: State<'_, Sharing>,
) -> Result<Option<PublisherStats>, ShareFailure> {
    let live = state.live.lock().await;
    let (Some(publisher), Some(screen)) = (live.publisher.as_ref(), live.screen.as_ref()) else {
        return Ok(None);
    };
    let mut stats = publisher.stats().await;
    stats.captured_frames = screen.capture.produced_frames();
    stats.encoded_frames = screen.capture.delivered_frames();
    stats.audio_samples = audio_samples(screen);
    Ok(Some(stats))
}

/// O mesmo para a camera (ADR-0038).
///
/// Os numeros de rede sao da **conexao**, que e uma so: bitrate, RTT e transporte
/// contam as duas publicacoes juntas, porque e assim que elas disputam a subida.
/// Os contadores de quadro sao desta captura, e so dela.
#[tauri::command]
pub async fn camera_stats(
    state: State<'_, Sharing>,
) -> Result<Option<PublisherStats>, ShareFailure> {
    let live = state.live.lock().await;
    let (Some(publisher), Some(camera)) = (live.publisher.as_ref(), live.camera.as_ref()) else {
        return Ok(None);
    };
    let mut stats = publisher.stats().await;
    #[cfg(target_os = "windows")]
    {
        stats.captured_frames = camera.capture.produced_frames();
        stats.encoded_frames = camera.capture.delivered_frames();
    }
    #[cfg(not(target_os = "windows"))]
    let _ = camera;
    stats.audio_samples = None;
    Ok(Some(stats))
}

#[cfg(target_os = "windows")]
fn audio_samples(screen: &ScreenActive) -> Option<u64> {
    screen
        .audio
        .as_ref()
        .map(crate::audio::AudioCapture::delivered_samples)
}

#[cfg(not(target_os = "windows"))]
fn audio_samples(_screen: &ScreenActive) -> Option<u64> {
    None
}

#[tauri::command]
pub async fn share_stop(state: State<'_, Sharing>) -> Result<(), ShareFailure> {
    let mut live = state.live.lock().await;
    release(&mut live, Source::Screen).await;
    Ok(())
}

/// As cameras que o Windows enxerga (ADR-0038).
#[tauri::command]
pub async fn camera_list() -> Result<Vec<CameraDeviceReport>, ShareFailure> {
    #[cfg(target_os = "windows")]
    {
        let devices = tauri::async_runtime::spawn_blocking(crate::camera::list_cameras)
            .await
            .unwrap_or_default();
        Ok(devices
            .into_iter()
            .map(|device| CameraDeviceReport {
                id: device.id,
                name: device.name,
            })
            .collect())
    }
    #[cfg(not(target_os = "windows"))]
    Ok(Vec::new())
}

/// Uma camera na lista do seletor.
#[derive(Debug, Clone, Serialize)]
pub struct CameraDeviceReport {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct StartCameraRequest {
    pub url: String,
    pub token: String,
    pub device_id: String,
}

/// Publishes the camera, on the connection the screen may already hold.
#[tauri::command]
pub async fn camera_start(
    app: AppHandle,
    state: State<'_, Sharing>,
    request: StartCameraRequest,
) -> Result<(), ShareFailure> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, state, request);
        return Err(ShareFailure::NoCameraHere);
    }

    #[cfg(target_os = "windows")]
    {
        let mut live = state.live.lock().await;
        if live.camera.is_some() {
            return Err(ShareFailure::AlreadyOnCamera);
        }

        let publisher = connect(&mut live, &app, &request.url, &request.token).await?;

        let sink = match publisher.publish_camera().await {
            Ok(sink) => sink,
            Err(error) => {
                live.close_if_idle().await;
                return Err(error.into());
            }
        };

        // O preview so nasce depois de publicar, e nunca antes: `Preview::stop`
        // espera a thread de codificacao terminar, e ela so termina quando o
        // ultimo `Tap` e descartado. Criado antes, um caminho de erro que ainda
        // segurasse o `Tap` travaria o processo aqui dentro.
        let (preview, tap) = preview::start(app.clone(), Source::Camera);
        let preview_control = preview.control();
        preview_control.set(true, GRID_FPS, false);

        let lost = app.clone();
        let capture = match crate::camera::start(
            &request.device_id,
            crate::publisher::CAMERA_SIZE,
            crate::publisher::CAMERA_FPS,
            sink,
            Some(tap),
            move |reason| give_up(&lost, Source::Camera, reason),
        ) {
            Ok(capture) => capture,
            Err(error) => {
                preview.stop();
                publisher.unpublish(Source::Camera).await;
                live.close_if_idle().await;
                return Err(error.into());
            }
        };

        live.camera = Some(CameraActive {
            capture,
            preview,
            preview_control,
        });
        Ok(())
    }
}

#[tauri::command]
pub async fn camera_stop(state: State<'_, Sharing>) -> Result<(), ShareFailure> {
    let mut live = state.live.lock().await;
    release(&mut live, Source::Camera).await;
    Ok(())
}

/// O mesmo controle de preview da tela, para a camera (ADR-0030).
#[tauri::command]
pub async fn camera_preview(
    state: State<'_, Sharing>,
    enabled: bool,
    fps: u32,
    focused: bool,
) -> Result<(), ShareFailure> {
    let live = state.live.lock().await;
    if let Some(camera) = live.camera.as_ref() {
        camera.preview_control.set(enabled, fps, focused);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn start_audio(
    sink: Option<livekit::webrtc::audio_source::native::NativeAudioSource>,
    kind: SourceKind,
    source_id: u64,
) -> (Option<crate::audio::AudioCapture>, Option<AudioModeReport>) {
    let Some(sink) = sink else {
        return (None, None);
    };
    match crate::audio::start(sink, audio_target(kind, source_id)) {
        Ok((capture, mode)) => {
            let report = match mode {
                crate::audio::AudioMode::ExcludingDiscord => AudioModeReport::ExcludingDiscord,
                crate::audio::AudioMode::OnlyWindow => AudioModeReport::OnlyWindow,
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

/// Sharing a window carries that window's sound, and nothing else (issue #11).
///
/// Uma tela inteira nao tem dono: o que se ve nela e a maquina toda, entao o que
/// sai e a maquina toda menos o Discord (ADR-0025). Uma janela tem — e capturar
/// so a arvore dela e o que dispensa silenciar as telas alheias aqui (ADR-0028).
/// Janela que sumiu entre listar e compartilhar cai no caminho de sempre.
#[cfg(target_os = "windows")]
fn audio_target(kind: SourceKind, source_id: u64) -> crate::audio::AudioTarget {
    match kind {
        SourceKind::Window => crate::audio::window_owner_pid(source_id)
            .map_or(crate::audio::AudioTarget::SystemExceptDiscord, |pid| {
                crate::audio::AudioTarget::Window(pid)
            }),
        SourceKind::Screen => crate::audio::AudioTarget::SystemExceptDiscord,
    }
}

#[cfg(not(target_os = "windows"))]
fn start_audio(
    _sink: Option<()>,
    _kind: SourceKind,
    _source_id: u64,
) -> ((), Option<AudioModeReport>) {
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
