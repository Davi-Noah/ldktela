//! Camera enumeration and the camera capture loop (ADR-0038).
//!
//! The screen has a capturer handed to us by libwebrtc; the camera does not.
//! The binding exposes `desktop_capturer` and nothing else, so this module is
//! the camera half of what `capture.rs` gets for free: enumerate the devices,
//! negotiate a format, pull frames, hand them to the encoder in NV12.
//!
//! **Media Foundation, and not DirectShow**, because MF is what current Windows
//! keeps working: it is the API behind the camera privacy setting, the frame
//! server that lets two applications read one device, and the built-in
//! converters this module leans on — `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`
//! is what turns whatever the webcam emits (MJPEG on most of them, YUY2 on the
//! rest) into the NV12 the encoder wants, without a decoder of ours in between.
//!
//! Like `capture.rs`, the loop is pull-based from a dedicated OS thread. Unlike
//! it, the clock belongs to the device: `ReadSample` blocks until the camera has
//! a frame, so the frame rate is negotiated once and then obeyed, instead of
//! being polled for.
//!
//! Human review zone (`CLAUDE.md` §10): OS media capture.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use livekit::webrtc::video_frame::{NV12Buffer, VideoFrame, VideoRotation};
use livekit::webrtc::video_source::native::NativeVideoSource;
use serde::{Deserialize, Serialize};
use windows::core::{Interface, GUID, PWSTR};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFActivate, IMFAttributes, IMFMediaSource, IMFMediaType, IMFSample,
    IMFSourceReader, MFCreateAttributes, MFCreateDeviceSource, MFCreateMediaType,
    MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources, MFShutdown, MFStartup,
    MFMediaType_Video, MFSTARTUP_FULL, MFVideoFormat_NV12, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};

use crate::capture::{fit, Size};

/// `HRESULT` of "another application already has the camera".
const ERROR_SHARING_VIOLATION: i32 = -2147024864; // 0x80070020
/// `HRESULT` of "the camera privacy setting says no".
const E_ACCESSDENIED: i32 = -2147024891; // 0x80070005
/// `MF_E_HW_MFT_FAILED_START_STREAMING`: the driver refused to start, which in
/// practice means the same thing as the sharing violation above.
const MF_E_HW_MFT_FAILED_START_STREAMING: i32 = -1072873339; // 0xC00D3E85
/// `MF_E_NO_MORE_TYPES`, the end of the format list. Not an error.
const MF_E_NO_MORE_TYPES: i32 = -1072875847; // 0xC00D36B9

/// One camera the person can choose, as the picker shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraDevice {
    /// The symbolic link. Stable across reboots and unique per physical device,
    /// which a friendly name is not: two identical webcams have one name.
    pub id: String,
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("a camera escolhida nao esta mais conectada")]
    Gone,
    #[error("outro aplicativo esta usando a camera")]
    Busy,
    #[error("o Windows nao deixa este aplicativo usar a camera")]
    NotAllowed,
    #[error("a camera nao oferece nenhum formato de video utilizavel")]
    NoUsableFormat,
    #[error("o Windows recusou a camera: {0}")]
    Platform(String),
}

impl CameraError {
    fn from_hresult(error: &windows::core::Error, context: &str) -> Self {
        match error.code().0 {
            ERROR_SHARING_VIOLATION | MF_E_HW_MFT_FAILED_START_STREAMING => Self::Busy,
            E_ACCESSDENIED => Self::NotAllowed,
            _ => Self::Platform(format!("{context}: {error}")),
        }
    }
}

/// Media Foundation, started for as long as this value lives.
///
/// `MFStartup` is reference counted per process, but COM apartment state is per
/// **thread**, so both are taken here and given back together: the enumeration
/// call and the capture thread each hold one, and neither has to know about the
/// other.
struct MfSession;

impl MfSession {
    fn new() -> Result<Self, CameraError> {
        unsafe {
            // Ja inicializado por outra parte do processo e um caso normal, e o
            // `HRESULT` de aviso nao e falha: so nao devolvemos a inicializacao
            // que nao fizemos, o que o `CoUninitialize` pareado resolve.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)
                .map_err(|e| CameraError::from_hresult(&e, "iniciando o Media Foundation"))?;
        }
        Ok(Self)
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

/// Attributes that say "video capture devices".
fn vidcap_attributes(extra: u32) -> Result<IMFAttributes, CameraError> {
    unsafe {
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 1 + extra)
            .map_err(|e| CameraError::from_hresult(&e, "criando atributos"))?;
        let attributes = attributes.ok_or(CameraError::NoUsableFormat)?;
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|e| CameraError::from_hresult(&e, "pedindo dispositivos de video"))?;
        Ok(attributes)
    }
}

/// Reads one `CoTaskMem` string attribute and frees it.
fn allocated_string(attributes: &IMFAttributes, key: &GUID) -> Option<String> {
    unsafe {
        let mut value = PWSTR::null();
        let mut length = 0u32;
        attributes
            .GetAllocatedString(key, &mut value, &mut length)
            .ok()?;
        let text = value.to_string().ok();
        CoTaskMemFree(Some(value.as_ptr().cast()));
        text
    }
}

/// Lists the cameras, in the order Windows reports them.
///
/// A camera that is busy still appears: it is connected, and saying so with a
/// clear failure when it is picked beats hiding a device the person can see in
/// every other application.
pub fn list_cameras() -> Vec<CameraDevice> {
    let Ok(_session) = MfSession::new() else {
        return Vec::new();
    };
    let Ok(attributes) = vidcap_attributes(0) else {
        return Vec::new();
    };

    let mut devices = Vec::new();
    unsafe {
        let mut raw: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        if MFEnumDeviceSources(&attributes, &mut raw, &mut count).is_err() {
            return devices;
        }

        for index in 0..count as usize {
            // Tira o ponteiro do array: a partir daqui quem libera e o `Drop` do
            // `IMFActivate`, e nao o `CoTaskMemFree` do array.
            let Some(activate) = std::ptr::read(raw.add(index)) else {
                continue;
            };
            let attributes: IMFAttributes = activate.cast().unwrap_or_else(|_| activate.into());
            let id = allocated_string(
                &attributes,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            );
            let name = allocated_string(&attributes, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME);
            if let Some(id) = id {
                devices.push(CameraDevice {
                    name: name.unwrap_or_else(|| "Câmera".to_owned()),
                    id,
                });
            }
        }
        CoTaskMemFree(Some(raw.cast()));
    }
    devices
}

/// Opens one camera by symbolic link, without enumerating again.
fn open_device(device_id: &str) -> Result<IMFMediaSource, CameraError> {
    let attributes = vidcap_attributes(1)?;
    unsafe {
        let wide: Vec<u16> = device_id.encode_utf16().chain(std::iter::once(0)).collect();
        attributes
            .SetString(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                PWSTR(wide.as_ptr() as *mut u16),
            )
            .map_err(|e| CameraError::from_hresult(&e, "escolhendo a camera"))?;

        MFCreateDeviceSource(&attributes).map_err(|e| match e.code().0 {
            // A camera foi desconectada entre listar e escolher.
            code if code == ERROR_SHARING_VIOLATION => CameraError::Busy,
            _ => {
                let mapped = CameraError::from_hresult(&e, "abrindo a camera");
                if matches!(mapped, CameraError::Platform(_)) {
                    CameraError::Gone
                } else {
                    mapped
                }
            }
        })
    }
}

/// One video format the device offers.
#[derive(Debug, Clone, Copy)]
struct Format {
    size: Size,
    fps: u32,
}

fn frame_size(media_type: &IMFMediaType) -> Option<Size> {
    let packed = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }.ok()?;
    Some(Size {
        width: (packed >> 32) as u32,
        height: (packed & 0xFFFF_FFFF) as u32,
    })
}

fn frame_rate(media_type: &IMFMediaType) -> Option<u32> {
    let packed = unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) }.ok()?;
    let numerator = (packed >> 32) as u32;
    let denominator = (packed & 0xFFFF_FFFF) as u32;
    (denominator > 0).then(|| numerator / denominator)
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
            // Abaixo do pedido e pior do que acima: 24 fps olhando para 30 é
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

/// Negotiates NV12 out of the reader, at the closest format the device has.
fn configure(reader: &IMFSourceReader, ceiling: Size, fps: u32) -> Result<Size, CameraError> {
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let mut offered = Vec::new();

    unsafe {
        for index in 0.. {
            match reader.GetNativeMediaType(stream, index) {
                Ok(media_type) => {
                    if let (Some(size), rate) = (frame_size(&media_type), frame_rate(&media_type)) {
                        if size.width >= 2 && size.height >= 2 {
                            offered.push(Format {
                                size,
                                fps: rate.unwrap_or(30),
                            });
                        }
                    }
                }
                Err(error) if error.code().0 == MF_E_NO_MORE_TYPES => break,
                Err(error) => return Err(CameraError::from_hresult(&error, "lendo os formatos")),
            }
        }
    }

    let chosen = choose_format(&offered, ceiling, fps).ok_or(CameraError::NoUsableFormat)?;

    unsafe {
        let wanted = MFCreateMediaType()
            .map_err(|e| CameraError::from_hresult(&e, "criando o formato de saida"))?;
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .and_then(|()| wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))
            .and_then(|()| {
                wanted.SetUINT64(
                    &MF_MT_FRAME_SIZE,
                    (u64::from(chosen.size.width) << 32) | u64::from(chosen.size.height),
                )
            })
            .and_then(|()| wanted.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(chosen.fps) << 32) | 1))
            .map_err(|e| CameraError::from_hresult(&e, "descrevendo o formato de saida"))?;

        // Com o processamento de video ligado, o leitor insere o conversor que
        // faltar — que e o caminho normal, porque quase nenhuma webcam entrega
        // NV12 direto. Se ainda assim recusar, tenta sem fixar tamanho e taxa:
        // resolucao errada e melhor do que camera nenhuma, e o escalador
        // conserta o tamanho depois.
        if reader.SetCurrentMediaType(stream, None, &wanted).is_err() {
            let loose = MFCreateMediaType()
                .map_err(|e| CameraError::from_hresult(&e, "criando o formato de saida"))?;
            loose
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .and_then(|()| loose.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))
                .map_err(|e| CameraError::from_hresult(&e, "descrevendo o formato de saida"))?;
            reader
                .SetCurrentMediaType(stream, None, &loose)
                .map_err(|e| CameraError::from_hresult(&e, "pedindo NV12 a camera"))?;
        }

        reader
            .SetStreamSelection(stream, true)
            .map_err(|e| CameraError::from_hresult(&e, "ligando a trilha de video"))?;

        // O tamanho real e o que o leitor diz depois de negociar, e nao o que
        // pedimos: ele pode ter aceitado outro.
        let current = reader
            .GetCurrentMediaType(stream)
            .map_err(|e| CameraError::from_hresult(&e, "lendo o formato negociado"))?;
        Ok(frame_size(&current).unwrap_or(chosen.size))
    }
}

/// A running camera capture.
pub struct CameraCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    produced: Arc<AtomicU64>,
    delivered: Arc<AtomicU64>,
}

impl CameraCapture {
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

/// Opens `device_id` and starts pushing frames into `sink`.
///
/// The device is opened **here**, on the caller's thread, so "this camera is in
/// use" is an error the button can show instead of a failure inside a thread
/// nobody is watching. Only the reading loop moves to its own thread.
pub fn start(
    device_id: &str,
    ceiling: Size,
    fps: u32,
    sink: NativeVideoSource,
    preview: Option<crate::preview::Tap>,
    on_lost: impl Fn() + Send + 'static,
) -> Result<CameraCapture, CameraError> {
    // A sessao de prova vive so o tempo de abrir: quem captura abre a sua, na
    // thread dela, porque o apartamento COM e por thread.
    {
        let _session = MfSession::new()?;
        let source = open_device(device_id)?;
        unsafe {
            let _ = source.Shutdown();
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let produced = Arc::new(AtomicU64::new(0));
    let delivered = Arc::new(AtomicU64::new(0));
    let counted_produced = Arc::clone(&produced);
    let counted_delivered = Arc::clone(&delivered);
    let device = device_id.to_owned();

    let thread = std::thread::Builder::new()
        .name("ldktela-camera".into())
        .spawn(move || {
            if let Err(error) = run(
                &device,
                ceiling,
                fps,
                sink,
                preview,
                counted_produced,
                counted_delivered,
                &thread_stop,
            ) {
                eprintln!("camera: captura interrompida: {error}");
                on_lost();
            }
        })
        .map_err(|_| CameraError::Platform("nao consegui criar a thread".into()))?;

    Ok(CameraCapture {
        stop,
        thread: Some(thread),
        produced,
        delivered,
    })
}

#[allow(clippy::too_many_arguments)]
fn run(
    device_id: &str,
    ceiling: Size,
    fps: u32,
    sink: NativeVideoSource,
    mut preview: Option<crate::preview::Tap>,
    produced: Arc<AtomicU64>,
    delivered: Arc<AtomicU64>,
    stop: &AtomicBool,
) -> Result<(), CameraError> {
    let _session = MfSession::new()?;
    let source = open_device(device_id)?;

    let reader = unsafe {
        let attributes = {
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attributes, 1)
                .map_err(|e| CameraError::from_hresult(&e, "criando atributos do leitor"))?;
            let attributes = attributes.ok_or(CameraError::NoUsableFormat)?;
            attributes
                .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
                .map_err(|e| CameraError::from_hresult(&e, "ligando a conversao de formato"))?;
            attributes
        };
        MFCreateSourceReaderFromMediaSource(&source, &attributes)
            .map_err(|e| CameraError::from_hresult(&e, "abrindo o leitor"))?
    };

    let captured = configure(&reader, ceiling, fps)?;
    let mut scratch = Frames::new(ceiling, sink, produced, delivered);
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

    while !stop.load(Ordering::Relaxed) {
        let mut flags = 0u32;
        let mut sample: Option<IMFSample> = None;
        unsafe {
            reader
                .ReadSample(
                    stream,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
                .map_err(|e| CameraError::from_hresult(&e, "lendo um quadro"))?;
        }

        if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
            return Err(CameraError::Gone);
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            // A camera foi desconectada. Nao e o mesmo que parar: quem
            // compartilha precisa saber.
            return Err(CameraError::Gone);
        }
        // Sem amostra e rotina: o leitor devolve vazio quando o formato mudou ou
        // quando o dispositivo ainda esta acordando.
        let Some(sample) = sample else {
            continue;
        };

        scratch.push(&sample, captured, preview.as_mut());
    }

    unsafe {
        let _ = source.Shutdown();
    }
    Ok(())
}

/// Keeps the conversion buffer between frames, like `capture::Scratch`.
struct Frames {
    ceiling: Size,
    sink: NativeVideoSource,
    buffer: Option<(Size, NV12Buffer)>,
    produced: Arc<AtomicU64>,
    delivered: Arc<AtomicU64>,
}

impl Frames {
    fn new(
        ceiling: Size,
        sink: NativeVideoSource,
        produced: Arc<AtomicU64>,
        delivered: Arc<AtomicU64>,
    ) -> Self {
        Self {
            ceiling,
            sink,
            buffer: None,
            produced,
            delivered,
        }
    }

    fn push(&mut self, sample: &IMFSample, size: Size, preview: Option<&mut crate::preview::Tap>) {
        if size.width < 2 || size.height < 2 {
            return;
        }
        if self.buffer.as_ref().is_none_or(|(had, _)| *had != size) {
            self.buffer = Some((size, NV12Buffer::new(size.width, size.height)));
        }
        let Some((_, nv12)) = self.buffer.as_mut() else {
            return;
        };

        // A amostra e emprestada pelo tempo do `Lock`, entao a copia acontece
        // aqui dentro e nada do Media Foundation escapa deste escopo.
        if !copy_nv12(sample, size, nv12) {
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
        if let Some(preview) = preview {
            preview.offer(nv12, size);
        }
    }
}

/// Copies one NV12 sample into `dst`, honouring both pitches.
///
/// The source pitch is rarely the width: drivers align rows, and a 1280-wide
/// frame routinely arrives with a 1536-byte stride. Copying it as if it were
/// packed shears the image diagonally — which looks like a broken encoder and
/// is not one.
fn copy_nv12(sample: &IMFSample, size: Size, dst: &mut NV12Buffer) -> bool {
    unsafe {
        let Ok(buffer) = sample.ConvertToContiguousBuffer() else {
            return false;
        };

        // `IMF2DBuffer` e o caminho certo quando existe: ele conhece o pitch. O
        // outro assume linhas coladas, que e o que um buffer contiguo entrega.
        if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
            let mut scanline = std::ptr::null_mut();
            let mut pitch = 0i32;
            if two_d.Lock2D(&mut scanline, &mut pitch).is_err() {
                return false;
            }
            let ok = pitch > 0 && {
                let pitch = pitch as usize;
                let rows = size.height as usize;
                let plane = pitch * rows;
                let total = plane + pitch * (rows / 2);
                let source = std::slice::from_raw_parts(scanline, total);
                planes_into(source, pitch, size, dst)
            };
            let _ = two_d.Unlock2D();
            return ok;
        }

        let mut data = std::ptr::null_mut();
        let mut length = 0u32;
        if buffer.Lock(&mut data, None, Some(&mut length)).is_err() {
            return false;
        }
        let pitch = size.width as usize;
        let needed = pitch * size.height as usize * 3 / 2;
        let ok = length as usize >= needed && {
            let source = std::slice::from_raw_parts(data, needed);
            planes_into(source, pitch, size, dst)
        };
        let _ = buffer.Unlock();
        ok
    }
}

/// Row-by-row copy of an NV12 frame with `pitch` into libwebrtc's buffer.
fn planes_into(source: &[u8], pitch: usize, size: Size, dst: &mut NV12Buffer) -> bool {
    let rows = size.height as usize;
    let width = size.width as usize;
    let chroma_rows = rows / 2;
    if pitch < width || source.len() < pitch * rows + pitch * chroma_rows {
        return false;
    }

    let (stride_y, stride_uv) = dst.strides();
    let (stride_y, stride_uv) = (stride_y as usize, stride_uv as usize);
    let (dst_y, dst_uv) = dst.data_mut();
    if stride_y < width || stride_uv < width {
        return false;
    }
    if dst_y.len() < stride_y * rows || dst_uv.len() < stride_uv * chroma_rows {
        return false;
    }

    for row in 0..rows {
        let from = &source[row * pitch..row * pitch + width];
        dst_y[row * stride_y..row * stride_y + width].copy_from_slice(from);
    }
    let uv = pitch * rows;
    for row in 0..chroma_rows {
        let from = &source[uv + row * pitch..uv + row * pitch + width];
        dst_uv[row * stride_uv..row * stride_uv + width].copy_from_slice(from);
    }
    true
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

    /// Um pitch maior que a largura é o caso comum, não a exceção: tratá-lo como
    /// colado inclina a imagem e parece defeito de encoder.
    #[test]
    fn a_padded_pitch_is_copied_without_shearing() {
        let size = Size {
            width: 4,
            height: 2,
        };
        let pitch = 8;
        let mut source = vec![0u8; pitch * 3];
        for row in 0..2 {
            for column in 0..4 {
                source[row * pitch + column] = (row * 4 + column) as u8;
            }
        }
        // Uma linha de croma para 2 de luma.
        for column in 0..4 {
            source[pitch * 2 + column] = 200 + column as u8;
        }

        let mut dst = NV12Buffer::new(size.width, size.height);
        assert!(planes_into(&source, pitch, size, &mut dst));

        let (stride_y, stride_uv) = dst.strides();
        let (y, uv) = dst.data_mut();
        assert_eq!(&y[0..4], &[0, 1, 2, 3]);
        assert_eq!(
            &y[stride_y as usize..stride_y as usize + 4],
            &[4, 5, 6, 7],
            "a segunda linha nao pode vir deslocada pelo padding"
        );
        assert_eq!(&uv[0..4], &[200, 201, 202, 203]);
        let _ = stride_uv;
    }

    #[test]
    fn a_short_frame_is_refused_instead_of_read_past_the_end() {
        let size = Size {
            width: 8,
            height: 4,
        };
        let mut dst = NV12Buffer::new(size.width, size.height);
        assert!(!planes_into(&[0u8; 8], 8, size, &mut dst));
    }
}
