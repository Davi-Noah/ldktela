//! Source enumeration and the screen capture loop (RF-37, ADR-0026).
//!
//! This is the module that replaced `getDisplayMedia`. Because the source list
//! is ours, the picker is ours; and because the WebView never asks for a
//! display, Chromium never draws its "you are sharing" bar.
//!
//! Capture is **pull-based**: `capture_frame()` is driven by our own clock, so
//! the publisher really does choose the frame rate (RF-36) instead of asking the
//! platform for one and hoping.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use livekit::webrtc::desktop_capturer::{
    CaptureError as CaptureFailure, DesktopCaptureSourceType, DesktopCapturer,
    DesktopCapturerOptions, DesktopFrame,
};
use livekit::webrtc::native::yuv_helper;
use livekit::webrtc::video_frame::{NV12Buffer, VideoFrame, VideoRotation};
use livekit::webrtc::video_source::native::NativeVideoSource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Screen,
    Window,
}

impl SourceKind {
    fn to_webrtc(self) -> DesktopCaptureSourceType {
        match self {
            SourceKind::Screen => DesktopCaptureSourceType::Screen,
            SourceKind::Window => DesktopCaptureSourceType::Window,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SourceKind::Screen => "tela",
            SourceKind::Window => "janela",
        }
    }
}

/// One entry in our own picker.
///
/// `id` travels as a string on purpose: on Windows a window source id is an
/// `HWND`, which is 64 bits, and JSON numbers in the WebView lose precision
/// above 2^53. Same reason `Snowflake` is a string everywhere else.
#[derive(Debug, Clone, Serialize)]
pub struct ShareSource {
    pub id: String,
    pub kind: SourceKind,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("o Windows nao ofereceu um capturador de {0}")]
    Unavailable(&'static str),
    #[error("a fonte escolhida nao existe mais")]
    SourceGone,
}

/// Lists what can be shared.
///
/// Monitors come back from libwebrtc with an empty title, so they are numbered
/// here. Windows with no title are skipped: they are overlays and tool windows
/// nobody means to share, and an untitled row in a picker is unusable.
pub fn list_sources(exclude_windows: &[u64]) -> Vec<ShareSource> {
    let mut sources = Vec::new();

    if let Some(capturer) = open(SourceKind::Screen) {
        for (index, source) in capturer.get_source_list().iter().enumerate() {
            sources.push(ShareSource {
                id: source.id().to_string(),
                kind: SourceKind::Screen,
                title: format!("Tela {}", index + 1),
            });
        }
    }

    if let Some(capturer) = open(SourceKind::Window) {
        for source in capturer.get_source_list() {
            let title = source.title();
            if title.trim().is_empty() || exclude_windows.contains(&source.id()) {
                continue;
            }
            sources.push(ShareSource {
                id: source.id().to_string(),
                kind: SourceKind::Window,
                title,
            });
        }
    }

    sources
}

fn open(kind: SourceKind) -> Option<DesktopCapturer> {
    let mut options = DesktopCapturerOptions::new(kind.to_webrtc());
    // O ponteiro faz parte do que se quer mostrar: sem ele, apontar para algo na
    // tela deixa de funcionar como comunicacao.
    options.set_include_cursor(true);
    DesktopCapturer::new(options)
}

/// The largest even-sided box that fits `source` inside `max` without changing
/// its shape.
///
/// Even on both sides because NV12 keeps one chroma sample per 2x2 block; an odd
/// side leaves half a block and libyuv rejects the buffer. Never upscales:
/// sending more pixels than were captured spends bitrate on nothing.
pub fn fit(source: Size, max: Size) -> Size {
    if source.width == 0 || source.height == 0 {
        return Size {
            width: 0,
            height: 0,
        };
    }
    let scale = f64::min(
        f64::from(max.width) / f64::from(source.width),
        f64::from(max.height) / f64::from(source.height),
    )
    .min(1.0);

    Size {
        width: even(((f64::from(source.width) * scale).round() as u32).max(2)),
        height: even(((f64::from(source.height) * scale).round() as u32).max(2)),
    }
}

fn even(value: u32) -> u32 {
    value - (value % 2)
}

/// A running capture.
pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Frames converted and offered to the encoder, whether or not it took them.
    produced: Arc<AtomicU64>,
    /// Frames the encoder actually accepted.
    ///
    /// The two differ for a reason worth knowing: libwebrtc's adapter refuses
    /// every frame while **no subscriber wants the track**, which is the normal
    /// state of a paused stream and indistinguishable from a dead capture unless
    /// both numbers are kept.
    delivered: Arc<AtomicU64>,
}

impl Capture {
    pub fn produced_frames(&self) -> u64 {
        self.produced.load(Ordering::Relaxed)
    }

    pub fn delivered_frames(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed)
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts pushing frames from `source_id` into `sink` at `fps`.
///
/// Runs on a dedicated OS thread rather than a task: `capture_frame` blocks, and
/// the capturer is affine to the thread that created it.
///
/// `on_lost` fires when the source goes away for good — the window was closed,
/// the monitor unplugged. Without it the share would sit there showing a frozen
/// last frame, which looks like our bug rather than a closed window.
pub fn start(
    kind: SourceKind,
    source_id: u64,
    max: Size,
    fps: u32,
    sink: NativeVideoSource,
    preview: Option<crate::preview::Tap>,
    on_lost: impl Fn() + Send + 'static,
) -> Result<Capture, CaptureError> {
    // A fonte e procurada aqui, na thread de quem chamou, para que "essa janela
    // nao existe mais" seja um erro do botao e nao uma falha silenciosa dentro
    // de uma thread que ninguem esta olhando.
    let probe = open(kind).ok_or(CaptureError::Unavailable(kind.label()))?;
    if !probe.get_source_list().iter().any(|s| s.id() == source_id) {
        return Err(CaptureError::SourceGone);
    }
    drop(probe);

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let delivered = Arc::new(AtomicU64::new(0));
    let produced = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&delivered);
    let counted_produced = Arc::clone(&produced);
    let period = Duration::from_secs_f64(1.0 / f64::from(fps.max(1)));

    let thread = std::thread::Builder::new()
        .name("ldkcord-capture".into())
        .spawn(move || {
            run(
                Job {
                    kind,
                    source_id,
                    max,
                    period,
                    delivered: counted,
                    produced: counted_produced,
                    preview,
                },
                sink,
                &thread_stop,
                on_lost,
            )
        })
        .map_err(|_| CaptureError::Unavailable("thread"))?;

    Ok(Capture {
        stop,
        thread: Some(thread),
        produced,
        delivered,
    })
}

/// What the capture thread needs to do its job, in one piece.
struct Job {
    kind: SourceKind,
    source_id: u64,
    max: Size,
    period: Duration,
    delivered: Arc<AtomicU64>,
    produced: Arc<AtomicU64>,
    /// ADR-0030. `None` quando nada quer ver esta captura de perto.
    preview: Option<crate::preview::Tap>,
}

fn run(job: Job, sink: NativeVideoSource, stop: &AtomicBool, on_lost: impl Fn() + Send + 'static) {
    let Job {
        kind,
        source_id,
        max,
        period,
        delivered,
        produced,
        preview,
    } = job;

    let Some(mut capturer) = open(kind) else {
        eprintln!("captura: o capturador sumiu entre escolher e comecar");
        on_lost();
        return;
    };
    let Some(source) = capturer
        .get_source_list()
        .into_iter()
        .find(|s| s.id() == source_id)
    else {
        eprintln!("captura: a fonte {source_id} sumiu entre escolher e comecar");
        on_lost();
        return;
    };

    let lost = Arc::new(AtomicBool::new(false));
    let callback_lost = Arc::clone(&lost);
    let mut scratch = Scratch::new(max, sink, delivered, produced, preview);
    capturer.start_capture(Some(source), move |result| match result {
        Ok(frame) => scratch.push(&frame),
        Err(CaptureFailure::Temporary) => {
            // Rotina: janela minimizada, jogo entrando em tela cheia exclusiva,
            // monitor suspendendo. Nao vale um evento na interface.
        }
        Err(CaptureFailure::Permanent) => {
            callback_lost.store(true, Ordering::Relaxed);
        }
    });

    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        capturer.capture_frame();
        if lost.load(Ordering::Relaxed) {
            eprintln!("captura: a fonte desapareceu");
            on_lost();
            return;
        }
        next += period;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            // Ficamos para tras. Recomeca a contagem em vez de tentar recuperar
            // o atraso com uma rajada de quadros que ninguem vai ver.
            next = now;
        }
    }
}

/// Keeps the conversion buffer between frames, so a steady stream of same-sized
/// frames allocates nothing.
struct Scratch {
    max: Size,
    sink: NativeVideoSource,
    buffer: Option<(Size, NV12Buffer)>,
    delivered: Arc<AtomicU64>,
    produced: Arc<AtomicU64>,
    preview: Option<crate::preview::Tap>,
}

impl Scratch {
    fn new(
        max: Size,
        sink: NativeVideoSource,
        delivered: Arc<AtomicU64>,
        produced: Arc<AtomicU64>,
        preview: Option<crate::preview::Tap>,
    ) -> Self {
        Self {
            max,
            sink,
            buffer: None,
            delivered,
            produced,
            preview,
        }
    }

    fn push(&mut self, frame: &DesktopFrame) {
        let (Ok(width), Ok(height)) = (u32::try_from(frame.width()), u32::try_from(frame.height()))
        else {
            return;
        };
        let captured = Size {
            width: even(width),
            height: even(height),
        };
        if captured.width < 2 || captured.height < 2 {
            return;
        }

        let stride = frame.stride();
        let data = frame.data();
        // O libyuv le `height * stride` bytes e entra em panico se faltar. Um
        // quadro curto e defeito do capturador; derrubar a transmissao por causa
        // dele seria pior do que pular o quadro.
        let needed = (stride as usize).saturating_mul(captured.height as usize);
        if stride < captured.width.saturating_mul(4) || data.len() < needed {
            eprintln!(
                "captura: quadro inconsistente ({}x{}, stride {stride}, {} bytes)",
                captured.width,
                captured.height,
                data.len()
            );
            return;
        }

        if self
            .buffer
            .as_ref()
            .is_none_or(|(size, _)| *size != captured)
        {
            self.buffer = Some((captured, NV12Buffer::new(captured.width, captured.height)));
        }
        let Some((_, nv12)) = self.buffer.as_mut() else {
            return;
        };

        let (stride_y, stride_uv) = nv12.strides();
        let (dst_y, dst_uv) = nv12.data_mut();
        yuv_helper::argb_to_nv12(
            data,
            stride,
            dst_y,
            stride_y,
            dst_uv,
            stride_uv,
            captured.width as i32,
            captured.height as i32,
        );

        // Sempre escala, mesmo quando o tamanho ja bate: `NV12Buffer` nao e
        // clonavel e o quadro precisa ser dono do seu buffer, entao a alternativa
        // seria alocar o scratch inteiro por quadro. Escalando, o que se aloca
        // por quadro tem o tamanho de saida — num monitor 4K indo a 1080p isso e
        // um quarto da memoria.
        let target = fit(captured, self.max);
        let buffer = nv12.scale(target.width as i32, target.height as i32);

        let accepted = self.sink.capture_frame(&VideoFrame {
            rotation: VideoRotation::VideoRotation0,
            timestamp_us: 0,
            buffer,
            frame_metadata: Default::default(),
        });
        self.produced.fetch_add(1, Ordering::Relaxed);
        if accepted {
            self.delivered.fetch_add(1, Ordering::Relaxed);
        }

        // Por ultimo, e nunca antes: o encoder ja recebeu o quadro dele. O
        // preview tem relogio proprio e descarta o que nao couber (ADR-0030).
        if let Some(preview) = self.preview.as_mut() {
            preview.offer(data, stride, captured);
        }
    }
}

/// Um unico quadro de uma fonte, para a miniatura do seletor (RF-37).
///
/// Abre um capturador so para isto e o fecha ao sair. Nao e barato, e nao
/// precisa ser: roda uma vez por fonte, enquanto o seletor esta aberto, e nunca
/// durante uma transmissao.
///
/// O primeiro `capture_frame` costuma voltar vazio — o DXGI ainda esta
/// acordando —, por isso a insistencia com teto. Sem o teto, uma fonte que
/// nunca entrega quadro prenderia a thread para sempre.
pub fn thumbnail(kind: SourceKind, source_id: u64, max: Size) -> Option<crate::preview::Raw> {
    const ATTEMPTS: u32 = 12;
    const WAIT: Duration = Duration::from_millis(25);

    let mut capturer = open(kind)?;
    let source = capturer
        .get_source_list()
        .into_iter()
        .find(|candidate| candidate.id() == source_id)?;

    let slot: Arc<std::sync::Mutex<Option<crate::preview::Raw>>> =
        Arc::new(std::sync::Mutex::new(None));
    let sink = Arc::clone(&slot);
    capturer.start_capture(Some(source), move |result| {
        let Ok(frame) = result else {
            return;
        };
        let (Ok(width), Ok(height)) = (u32::try_from(frame.width()), u32::try_from(frame.height()))
        else {
            return;
        };
        let captured = Size {
            width: even(width),
            height: even(height),
        };
        let target = fit(captured, max);
        let Some(pixels) =
            crate::preview::subsample(frame.data(), frame.stride(), captured, target)
        else {
            return;
        };
        if let Ok(mut guard) = sink.lock() {
            *guard = Some(crate::preview::Raw {
                width: target.width,
                height: target.height,
                pixels,
            });
        }
    });

    for _ in 0..ATTEMPTS {
        capturer.capture_frame();
        if slot.lock().is_ok_and(|guard| guard.is_some()) {
            break;
        }
        std::thread::sleep(WAIT);
    }

    slot.lock().ok().and_then(|mut guard| guard.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real screen, so it is not part of `just check`.
    ///
    /// Run it by hand, from `desktop/src-tauri`:
    ///
    /// ```text
    /// cargo test -- --ignored --nocapture
    /// ```
    ///
    /// It answers the one question the interface cannot: whether frames are
    /// actually reaching the encoder. A publisher panel showing 0 fps looks the
    /// same whether the capture is dead or the stream is paused downstream, and
    /// those have nothing to do with each other.
    /// `tokio::test` nao e detalhe: `NativeVideoSource::new` semeia um quadro
    /// preto de keepalive com `tokio::spawn`, entao construir a fonte fora de um
    /// runtime entra em panico antes de qualquer captura.
    #[tokio::test]
    #[ignore]
    async fn the_screen_really_produces_frames() {
        let sources = list_sources(&[]);
        for source in &sources {
            println!("fonte {:?} {} {:?}", source.kind, source.id, source.title);
        }
        let screen = sources
            .iter()
            .find(|s| s.kind == SourceKind::Screen)
            .expect("deve existir pelo menos uma tela");
        let id: u64 = screen.id.parse().expect("id numerico");

        let sink = NativeVideoSource::new(
            livekit::webrtc::video_source::VideoResolution {
                width: 1920,
                height: 1080,
            },
            true,
        );
        let capture = start(
            SourceKind::Screen,
            id,
            Size {
                width: 1920,
                height: 1080,
            },
            30,
            sink,
            None,
            || eprintln!("fonte perdida"),
        )
        .expect("a captura deve iniciar");

        tokio::time::sleep(Duration::from_secs(2)).await;
        let produced = capture.produced_frames();
        capture.stop();

        // `produced`, e nao `delivered`: sem ninguem assinando a track, o
        // adaptador do libwebrtc recusa todo quadro, e exigir aceitacao aqui
        // testaria o SFU em vez da captura. Quem cobre a outra ponta e o
        // `a_real_screen_reaches_the_sfu`.
        println!("quadros produzidos em 2 s: {produced} (esperado ~60)");
        assert!(
            produced > 10,
            "a captura abriu mas nao produziu quadro nenhum"
        );
    }

    /// Toca a tela de verdade, como o teste acima. Rode a mao:
    ///
    /// ```text
    /// cargo test -- --ignored --nocapture
    /// ```
    ///
    /// Responde a unica pergunta que um teste de unidade nao alcanca: se o
    /// `DesktopCapturer` entrega quadro para uma fonte escolhida a frio, sem
    /// laco de captura rodando — que e exatamente o que o seletor faz ao abrir.
    #[test]
    #[ignore]
    fn a_real_screen_produces_a_thumbnail() {
        let sources = list_sources(&[]);
        let screen = sources
            .iter()
            .find(|s| s.kind == SourceKind::Screen)
            .expect("deve existir pelo menos uma tela");
        let id: u64 = screen.id.parse().expect("id numerico");

        let frame = thumbnail(SourceKind::Screen, id, crate::preview::THUMBNAIL_MAX)
            .expect("a tela deve entregar um quadro");
        println!("miniatura: {}x{}", frame.width, frame.height);
        assert!(frame.width <= crate::preview::THUMBNAIL_MAX.width);
        assert!(frame.height <= crate::preview::THUMBNAIL_MAX.height);
        assert_eq!(
            frame.pixels.len(),
            (frame.width as usize) * (frame.height as usize) * 4
        );

        let url = crate::preview::encode_data_url(&frame).expect("deve virar data URL");
        println!("data URL de {} bytes", url.len());
        assert!(url.starts_with("data:image/jpeg;base64,"));
    }

    fn size(width: u32, height: u32) -> Size {
        Size { width, height }
    }

    #[test]
    fn a_4k_monitor_is_scaled_down_to_the_chosen_ladder() {
        assert_eq!(fit(size(3840, 2160), size(1920, 1080)), size(1920, 1080));
    }

    #[test]
    fn a_source_smaller_than_the_ladder_is_never_upscaled() {
        assert_eq!(fit(size(1280, 720), size(1920, 1080)), size(1280, 720));
    }

    #[test]
    fn an_ultrawide_keeps_its_shape_and_is_bounded_by_the_width() {
        assert_eq!(fit(size(3440, 1440), size(1920, 1080)), size(1920, 804));
    }

    #[test]
    fn a_tall_window_is_bounded_by_the_height() {
        // 1400 * (1080/1400) = 1080 exato; a largura e que sobra do arredondamento.
        assert_eq!(fit(size(600, 1400), size(1920, 1080)), size(462, 1080));
    }

    #[test]
    fn every_side_comes_out_even() {
        // NV12 guarda uma amostra de croma por bloco 2x2: lado impar deixa meio
        // bloco e o libyuv recusa o buffer.
        for (w, h) in [(1365, 767), (999, 333), (3, 3), (1921, 1081)] {
            let fitted = fit(size(w, h), size(1920, 1080));
            assert_eq!(fitted.width % 2, 0, "largura impar para {w}x{h}");
            assert_eq!(fitted.height % 2, 0, "altura impar para {w}x{h}");
        }
    }

    #[test]
    fn a_degenerate_source_does_not_produce_a_zero_sided_frame() {
        assert_eq!(fit(size(0, 0), size(1920, 1080)), size(0, 0));
        assert_eq!(fit(size(1, 1), size(1920, 1080)), size(2, 2));
    }
}
