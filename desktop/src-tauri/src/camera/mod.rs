//! Camera enumeration and the camera capture loop (ADR-0038, ADR-0039).
//!
//! The screen has a capturer handed to us by libwebrtc; the camera does not.
//! The binding exposes `desktop_capturer` and nothing else, so this module is
//! the camera half of what `capture.rs` gets for free: enumerate the devices,
//! negotiate a format, pull frames, hand them to the encoder in NV12.
//!
//! **Two paths into Windows, with Media Foundation in front** (ADR-0039). MF is
//! what current Windows keeps working: it is the API behind the camera privacy
//! setting, the frame server that lets two applications read one device, and the
//! source of the converters that spare us a decoder. What it is not is
//! universal — virtual cameras (DroidCam, OBS, Iriun) show up in its
//! enumeration and refuse to open, and those are the cameras this audience
//! actually uses. So `dshow` exists, and `mf` is tried first.
//!
//! The split is contained here. `list_cameras` returns **one** list with each
//! device once, and `start` decides which path opens it. Nothing above this
//! module knows there are two.
//!
//! Human review zone (`CLAUDE.md` §10): OS media capture.

mod convert;
mod dshow;
mod mf;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use livekit::webrtc::video_frame::{NV12Buffer, VideoFrame, VideoRotation};
use livekit::webrtc::video_source::native::NativeVideoSource;
use serde::{Deserialize, Serialize};

use crate::capture::{fit, Size};

/// One camera the person can choose, as the picker shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraDevice {
    /// The handle that reopens this device: a Media Foundation symbolic link,
    /// or a DirectShow moniker name when only DirectShow has it. Both are
    /// stable across reboots and unique per device, which a friendly name is
    /// not — two identical webcams share one name.
    pub id: String,
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("a camera escolhida nao esta mais conectada")]
    Gone,
    /// Media Foundation enumerated the device and then refused to open it.
    ///
    /// Only ever reaches the interface when DirectShow could not pick it up
    /// either, so the text says "não abriu" and names the code Media Foundation
    /// gave — it is the preferred path, and its code is the one worth searching
    /// for (ADR-0039, decision 5).
    #[error("o Windows nao abriu esta camera por nenhum caminho (Media Foundation: {0:#010x})")]
    NoPathOpensIt(u32),
    #[error("outro aplicativo esta usando a camera")]
    Busy,
    #[error("o Windows nao deixa este aplicativo usar a camera")]
    NotAllowed,
    #[error("a camera nao oferece nenhum formato de video utilizavel")]
    NoUsableFormat,
    #[error(
        "esta camera so entrega video comprimido, que este aplicativo nao decodifica. Tente baixar a resolucao nas configuracoes dela"
    )]
    OnlyCompressed,
    #[error("o Windows recusou a camera: {0}")]
    Platform(String),
}

impl CameraError {
    /// Whether this failure is the shape that a DirectShow-only camera makes.
    ///
    /// `Gone` counts: a virtual camera whose Media Foundation registration is
    /// stale reports exactly that, and it is the DirectShow side that still
    /// works. `Busy` and `NotAllowed` do not — the device answered, and trying
    /// the other path would only turn a clear message into a vague one.
    fn may_be_directshow_only(&self) -> bool {
        matches!(self, Self::Gone | Self::NoPathOpensIt(_))
    }
}

/// Lists every camera on the machine, once each.
///
/// Both paths enumerate and the results are merged (ADR-0039, decisions 2 and
/// 3). A device the two agree on keeps its Media Foundation handle, because
/// that is the path `start` tries first anyway.
///
/// A camera that is busy still appears: it is connected, and saying so with a
/// clear failure when it is picked beats hiding a device the person can see in
/// every other application.
pub fn list_cameras() -> Vec<CameraDevice> {
    let native = mf::list();
    let mut devices: Vec<CameraDevice> = native.clone();

    let keys: HashSet<String> = native.iter().filter_map(|d| device_key(&d.id)).collect();
    let names: HashSet<&str> = native.iter().map(|d| d.name.as_str()).collect();

    for device in dshow::list() {
        let key = device.path.as_deref().and_then(device_key);
        let already =
            key.is_some_and(|k| keys.contains(&k)) || names.contains(device.name.as_str());
        if already {
            continue;
        }
        devices.push(CameraDevice {
            id: device.moniker,
            name: device.name,
        });
    }
    devices
}

/// Whether this handle names a DirectShow moniker rather than an MF link.
///
/// No prefix of our own: the two namespaces are already disjoint — a moniker
/// name starts with `@device:` and a symbolic link with `\\?\` — and inventing
/// an encoding would mean migrating the device someone already picked.
fn is_directshow(id: &str) -> bool {
    id.starts_with("@device:")
}

/// The part of a device path that identifies the hardware instance.
///
/// The same camera has different last segments in the two APIs — Media
/// Foundation names its own interface GUID where DirectShow names the capture
/// category — so everything up to the last `#` is what they agree on.
fn device_key(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let path = lower.strip_prefix("@device:pnp:").unwrap_or(lower.as_str());
    let cut = path.rfind('#')?;
    let key = &path[..cut];
    // At least two segments, or this is not a device path at all.
    key.contains('#').then(|| key.to_owned())
}

/// One video format a device offers.
#[derive(Debug, Clone, Copy)]
struct Format {
    size: Size,
    fps: u32,
}

/// Picks the format to ask the camera for.
///
/// Prefers the largest frame that still fits the ceiling — upscaling a 640x480
/// webcam to 720p spends bitrate on pixels the sensor never produced — and then
/// the frame rate closest to what was asked without going under it. A device
/// that offers nothing small enough falls back to its smallest format, which the
/// scaler then brings down.
fn choose_format(offered: &[Format], ceiling: Size, fps: u32) -> Option<Format> {
    let fits = |f: &&Format| f.size.width <= ceiling.width && f.size.height <= ceiling.height;
    let area = |f: &Format| u64::from(f.size.width) * u64::from(f.size.height);

    let pool: Vec<&Format> = offered.iter().filter(fits).collect();
    if pool.is_empty() {
        return offered.iter().min_by_key(|f| area(f)).copied();
    }

    let best_area = pool.iter().map(|f| area(f)).max()?;
    pool.into_iter()
        .filter(|f| area(f) == best_area)
        .min_by_key(|f| {
            // Abaixo do pedido é pior do que acima: 24 fps olhando para 30 é
            // perceptível, e 60 quando se pediu 30 só custa o que o encoder já
            // ia descartar.
            if f.fps >= fps {
                (0u32, f.fps - fps)
            } else {
                (1u32, fps - f.fps)
            }
        })
        .copied()
}

/// Where both capture paths end: convert, scale, hand to the encoder.
///
/// Media Foundation arrives here from a loop of ours; DirectShow arrives from
/// the device's own streaming thread (ADR-0039, decision 9). Keeping the two
/// together is what stops the difference from spreading.
pub(crate) struct Frames {
    ceiling: Size,
    sink: NativeVideoSource,
    preview: Option<crate::preview::Tap>,
    buffer: Option<(Size, NV12Buffer)>,
    produced: Arc<AtomicU64>,
    delivered: Arc<AtomicU64>,
}

impl Frames {
    fn new(
        ceiling: Size,
        sink: NativeVideoSource,
        preview: Option<crate::preview::Tap>,
        produced: Arc<AtomicU64>,
        delivered: Arc<AtomicU64>,
    ) -> Self {
        Self {
            ceiling,
            sink,
            preview,
            buffer: None,
            produced,
            delivered,
        }
    }

    /// Runs `write` into a reusable NV12 buffer, then scales and publishes it.
    ///
    /// `write` returning false means the frame was unusable — a short buffer, an
    /// odd size — and nothing is counted or sent. One frame skipped is not a
    /// reason to tear the capture down.
    pub(crate) fn deliver(&mut self, size: Size, write: impl FnOnce(&mut NV12Buffer) -> bool) {
        if size.width < 2 || size.height < 2 {
            return;
        }
        if self.buffer.as_ref().is_none_or(|(had, _)| *had != size) {
            self.buffer = Some((size, NV12Buffer::new(size.width, size.height)));
        }
        let Some((_, nv12)) = self.buffer.as_mut() else {
            return;
        };
        if !write(nv12) {
            return;
        }

        let target = fit(size, self.ceiling);
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

        // Depois do encoder, sempre (ADR-0030).
        if let Some(preview) = self.preview.as_mut() {
            preview.offer(nv12, size);
        }
    }
}

/// A running camera capture, on whichever path opened it.
pub struct CameraCapture {
    running: Running,
    produced: Arc<AtomicU64>,
    delivered: Arc<AtomicU64>,
}

enum Running {
    MediaFoundation(mf::Capture),
    DirectShow(dshow::Capture),
}

impl CameraCapture {
    pub fn produced_frames(&self) -> u64 {
        self.produced.load(Ordering::Relaxed)
    }

    pub fn delivered_frames(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed)
    }

    pub fn stop(self) {
        match self.running {
            Running::MediaFoundation(capture) => capture.stop(),
            Running::DirectShow(capture) => capture.stop(),
        }
    }
}

/// Opens `device_id` and starts pushing frames into `sink`.
///
/// Media Foundation is tried first, and a device it enumerated but will not open
/// falls through to DirectShow without telling anyone (ADR-0039, decision 5):
/// whoever picked a camera wants the camera, not a lecture about which Windows
/// API reached it. When both refuse, the error reported is Media Foundation's —
/// it is the preferred path, and its code is the one worth searching for.
///
/// The device is opened **before** returning, so "this camera is in use" is an
/// error the button can show instead of a failure inside a thread nobody is
/// watching.
pub fn start(
    device_id: &str,
    ceiling: Size,
    fps: u32,
    sink: NativeVideoSource,
    preview: Option<crate::preview::Tap>,
    on_lost: impl Fn() + Send + 'static,
) -> Result<CameraCapture, CameraError> {
    let produced = Arc::new(AtomicU64::new(0));
    let delivered = Arc::new(AtomicU64::new(0));
    let frames = Frames::new(
        ceiling,
        sink,
        preview,
        Arc::clone(&produced),
        Arc::clone(&delivered),
    );
    let finish = |running| CameraCapture {
        running,
        produced,
        delivered,
    };

    if is_directshow(device_id) {
        return dshow::start(device_id, ceiling, fps, frames, on_lost)
            .map(|c| finish(Running::DirectShow(c)));
    }

    let refused = match mf::probe(device_id) {
        Ok(()) => {
            return mf::start(device_id, ceiling, fps, frames, on_lost)
                .map(|c| finish(Running::MediaFoundation(c)))
        }
        Err(error) if error.may_be_directshow_only() => error,
        Err(error) => return Err(error),
    };

    let Some(moniker) = dshow::counterpart(device_id) else {
        return Err(refused);
    };
    // O HRESULT ja saiu no log de quem recusou; repeti-lo aqui so faria a linha
    // dizer "nao abriu por nenhum caminho" logo antes de tentar o outro.
    eprintln!("camera: o Media Foundation recusou; tentando pelo DirectShow");
    match dshow::start(&moniker, ceiling, fps, frames, on_lost) {
        Ok(capture) => Ok(finish(Running::DirectShow(capture))),
        Err(error) => {
            eprintln!("camera: o DirectShow tambem recusou: {error}");
            Err(refused)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(width: u32, height: u32, fps: u32) -> Format {
        Format {
            size: Size { width, height },
            fps,
        }
    }

    const HD: Size = Size {
        width: 1280,
        height: 720,
    };

    #[test]
    fn the_largest_format_that_fits_wins() {
        let offered = [
            format(640, 480, 30),
            format(1280, 720, 30),
            format(1920, 1080, 30),
        ];
        let chosen = choose_format(&offered, HD, 30).expect("algum formato");
        assert_eq!(chosen.size, HD, "1080p passa do teto e 480p desperdica");
    }

    /// Esticar 480p para 720p gasta bitrate em pixels que o sensor nunca viu.
    #[test]
    fn a_camera_smaller_than_the_ceiling_is_taken_as_it_is() {
        let offered = [format(640, 480, 30)];
        let chosen = choose_format(&offered, HD, 30).expect("algum formato");
        assert_eq!(chosen.size.width, 640);
    }

    #[test]
    fn nothing_small_enough_falls_back_to_the_smallest() {
        let offered = [format(1920, 1080, 30), format(2560, 1440, 30)];
        let chosen = choose_format(&offered, HD, 30).expect("algum formato");
        assert_eq!(chosen.size.width, 1920, "o escalador cuida do resto");
    }

    #[test]
    fn a_rate_above_the_asked_one_beats_a_rate_below_it() {
        let offered = [format(1280, 720, 24), format(1280, 720, 60)];
        let chosen = choose_format(&offered, HD, 30).expect("algum formato");
        assert_eq!(chosen.fps, 60, "24 fps onde se pediu 30 se ve");
    }

    #[test]
    fn the_exact_rate_wins_when_it_exists() {
        let offered = [
            format(1280, 720, 60),
            format(1280, 720, 30),
            format(1280, 720, 15),
        ];
        let chosen = choose_format(&offered, HD, 30).expect("algum formato");
        assert_eq!(chosen.fps, 30);
    }

    #[test]
    fn a_camera_that_offers_nothing_is_not_a_panic() {
        assert!(choose_format(&[], HD, 30).is_none());
    }

    /// O mesmo dispositivo tem GUID de interface diferente nas duas APIs. Casar
    /// a string inteira listaria toda camera fisica duas vezes.
    #[test]
    fn the_two_apis_name_the_same_device_differently_and_still_match() {
        let mf = r"\\?\usb#vid_046d&pid_0825&mi_00#7&1e2c3a4b&0&0000#{e5323777-f976-4f5b-9b55-b94699c46e44}\global";
        let dshow = r"@device:pnp:\\?\usb#vid_046d&pid_0825&mi_00#7&1e2c3a4b&0&0000#{65e8773d-8f56-11d0-a3b9-00a0c9223196}\global";
        assert_eq!(device_key(mf), device_key(dshow));
        assert!(device_key(mf).is_some());
    }

    #[test]
    fn two_different_cameras_do_not_collide() {
        let one = r"\\?\usb#vid_046d&pid_0825&mi_00#7&aaaa&0&0000#{e5323777-f976-4f5b-9b55-b94699c46e44}\global";
        let other = r"\\?\usb#vid_046d&pid_0825&mi_00#7&bbbb&0&0000#{e5323777-f976-4f5b-9b55-b94699c46e44}\global";
        assert_ne!(device_key(one), device_key(other));
    }

    /// Uma camera virtual registrada como software nao tem caminho de
    /// dispositivo. Inventar uma chave para ela faria duas delas colidirem.
    #[test]
    fn a_software_moniker_has_no_device_key() {
        assert_eq!(
            device_key("@device:sw:{860BB310-5D01-11D0-BD3B-00A0C911CE86}\\{A3FCE0F5-3493-419F-958A-ABA1250EC20B}"),
            None
        );
        assert_eq!(device_key("Camera"), None);
    }

    #[test]
    fn the_two_kinds_of_handle_are_told_apart_without_a_prefix() {
        assert!(is_directshow("@device:pnp:\\\\?\\usb#vid_046d"));
        assert!(!is_directshow(r"\\?\usb#vid_046d&pid_0825"));
    }

    /// Camera ocupada nao vira tentativa no outro caminho: o dispositivo
    /// respondeu, e trocar a mensagem clara por uma vaga seria piorar.
    #[test]
    fn only_a_refusal_that_looks_like_a_virtual_camera_falls_through() {
        assert!(CameraError::Gone.may_be_directshow_only());
        assert!(CameraError::NoPathOpensIt(0x8007_0057).may_be_directshow_only());
        assert!(!CameraError::Busy.may_be_directshow_only());
        assert!(!CameraError::NotAllowed.may_be_directshow_only());
    }

    /// Diagnostico: que caminho abre cada camera desta maquina.
    ///
    /// `#[ignore]` porque depende de hardware. E o unico jeito de ver a fusao
    /// das duas listas acontecendo, e de saber se uma camera que falha no Media
    /// Foundation encontra o par dela no DirectShow.
    ///
    /// Rode com: `cargo test --lib camera -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn which_path_opens_each_camera() {
        println!("-- Media Foundation --");
        for device in mf::list() {
            let verdict = match mf::probe(&device.id) {
                Ok(()) => "abre".to_owned(),
                Err(error) => format!("{error}"),
            };
            println!("  {} => {verdict}", device.name);
        }

        println!("-- DirectShow --");
        for device in dshow::list() {
            println!(
                "  {} (caminho: {})",
                device.name,
                device.path.as_deref().unwrap_or("nenhum, e software")
            );
        }

        println!("-- lista unificada, que e a que o menu mostra --");
        for device in list_cameras() {
            let path = if is_directshow(&device.id) {
                "DirectShow".to_owned()
            } else {
                match mf::probe(&device.id) {
                    Ok(()) => "Media Foundation".to_owned(),
                    Err(_) => match dshow::counterpart(&device.id) {
                        Some(_) => "Media Foundation recusou, cai para o DirectShow".to_owned(),
                        None => "nenhum caminho abre".to_owned(),
                    },
                }
            };
            println!("  {} => {path}", device.name);
        }
    }

    /// Abre cada camera desta maquina e conta quadros.
    ///
    /// `#[ignore]` e depende de hardware, como o teste de SFU real do
    /// `publisher.rs`: e a unica forma de provar que a negociacao de formato, o
    /// grafo do DirectShow e a conversao de cor funcionam contra um driver de
    /// verdade, e nenhum teste sem camera prova isso.
    ///
    /// `tokio::test` porque `NativeVideoSource` exige um reator: a fonte nativa
    /// do libwebrtc agenda nele, e fora de um runtime ela entra em panico ao ser
    /// criada.
    #[tokio::test]
    #[ignore]
    async fn a_real_camera_delivers_frames() {
        use livekit::webrtc::video_source::VideoResolution;
        use std::time::Duration;

        let devices = list_cameras();
        if devices.is_empty() {
            eprintln!("sem camera nesta maquina; nada a provar");
            return;
        }

        let ceiling = Size {
            width: 1280,
            height: 720,
        };
        let mut delivered_by = Vec::new();

        for device in devices {
            let sink = NativeVideoSource::new(
                VideoResolution {
                    width: ceiling.width,
                    height: ceiling.height,
                },
                false,
            );
            let capture = match start(&device.id, ceiling, 30, sink, None, || {
                eprintln!("camera: a fonte sumiu durante o teste");
            }) {
                Ok(capture) => capture,
                Err(error) => {
                    // Ocupada ou bloqueada e resultado legitimo do ambiente, e o
                    // caminho de erro tambem e o que se quer exercitar.
                    println!("{}: nao abriu ({error})", device.name);
                    continue;
                }
            };

            tokio::time::sleep(Duration::from_secs(2)).await;
            let produced = capture.produced_frames();
            capture.stop();

            println!("{}: {produced} quadros em 2 s", device.name);
            if produced > 0 {
                delivered_by.push(device.name);
            }
        }

        assert!(
            !delivered_by.is_empty(),
            "nenhuma camera desta maquina entregou um quadro sequer"
        );
    }
}
