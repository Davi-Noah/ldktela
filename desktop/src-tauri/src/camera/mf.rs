//! The Media Foundation capture path (ADR-0038, preferred by ADR-0039).
//!
//! Pull-based from a dedicated OS thread, like `capture.rs`. Unlike it, the
//! clock belongs to the device: `ReadSample` blocks until the camera has a
//! frame, so the frame rate is negotiated once and then obeyed, instead of being
//! polled for.
//!
//! The format conversion is Windows': with
//! `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING` the reader turns whatever the
//! webcam emits — MJPEG on most of them, YUY2 on the rest — into the NV12 the
//! encoder wants, with no decoder of ours in between. That is the main thing
//! this path has that the DirectShow one does not.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use livekit::webrtc::video_frame::NV12Buffer;
use windows::core::{Interface, GUID, PWSTR};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFActivate, IMFAttributes, IMFMediaSource, IMFMediaType, IMFSample,
    IMFSourceReader, MFCreateAttributes, MFCreateDeviceSource, MFCreateMediaType,
    MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources, MFMediaType_Video, MFShutdown,
    MFStartup, MFVideoFormat_NV12, MFSTARTUP_FULL, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};

use super::convert::nv12_into;
use super::{choose_format, CameraDevice, CameraError, Format, Frames, OnLost};
use crate::capture::Size;

/// `HRESULT` of "another application already has the camera".
const ERROR_SHARING_VIOLATION: i32 = -2147024864; // 0x80070020
/// `HRESULT` of "the camera privacy setting says no".
const E_ACCESSDENIED: i32 = -2147024891; // 0x80070005
/// `MF_E_HW_MFT_FAILED_START_STREAMING`: the driver refused to start, which in
/// practice means the same thing as the sharing violation above.
const MF_E_HW_MFT_FAILED_START_STREAMING: i32 = -1072873339; // 0xC00D3E85
/// `MF_E_NO_MORE_TYPES`, the end of the format list. Not an error.
const MF_E_NO_MORE_TYPES: i32 = -1072875847; // 0xC00D36B9
/// The symbolic link no longer names a device: unplugged between listing and
/// choosing, or a virtual camera whose source went away.
const ERROR_FILE_NOT_FOUND: i32 = -2147024894; // 0x80070002
const MF_E_NOT_FOUND: i32 = -1072875819; // 0xC00D36D5
const E_INVALIDARG: i32 = -2147024809; // 0x80070057

impl CameraError {
    pub(super) fn from_hresult(error: &windows::core::Error, context: &str) -> Self {
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
struct Session;

impl Session {
    fn new() -> Result<Self, CameraError> {
        unsafe {
            // Já inicializado por outra parte do processo é um caso normal, e o
            // `HRESULT` de aviso não é falha: só não devolvemos a inicialização
            // que não fizemos, o que o `CoUninitialize` pareado resolve.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)
                .map_err(|e| CameraError::from_hresult(&e, "iniciando o Media Foundation"))?;
        }
        Ok(Self)
    }
}

impl Drop for Session {
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

/// Lists the cameras Media Foundation knows, in the order Windows reports them.
pub(super) fn list() -> Vec<CameraDevice> {
    let Ok(_session) = Session::new() else {
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
            // Tira o ponteiro do array: a partir daqui quem libera é o `Drop` do
            // `IMFActivate`, e não o `CoTaskMemFree` do array.
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

/// The friendly name Media Foundation gives `device_id`, if it still has it.
pub(super) fn name_of(device_id: &str) -> Option<String> {
    list()
        .into_iter()
        .find(|d| d.id == device_id)
        .map(|d| d.name)
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

        MFCreateDeviceSource(&attributes).map_err(|e| {
            let code = e.code().0;
            eprintln!("camera: MFCreateDeviceSource recusou com {code:#010x}");
            match code {
                // Sumiu entre listar e escolher — ou é uma câmera virtual cuja
                // inscrição no MF ficou para trás. Os dois casos merecem a
                // tentativa pelo DirectShow (ADR-0039).
                ERROR_FILE_NOT_FOUND | MF_E_NOT_FOUND => CameraError::Gone,
                // O dispositivo **está** ali — a enumeração acabou de devolvê-lo
                // — e a ativação recusa mesmo assim. É o que uma câmera virtual
                // só-DirectShow faz: aparece no MF e não abre por ele.
                E_INVALIDARG => CameraError::NoPathOpensIt(code as u32),
                _ => CameraError::from_hresult(&e, "abrindo a camera"),
            }
        })
    }
}

/// Opens and immediately closes `device_id`, to learn whether this path works.
///
/// Costs one open, and buys the answer to "does Media Foundation serve this
/// camera" before any thread is spawned or any fallback is chosen.
pub(super) fn probe(device_id: &str) -> Result<(), CameraError> {
    let _session = Session::new()?;
    let source = open_device(device_id)?;
    unsafe {
        let _ = source.Shutdown();
    }
    Ok(())
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

        // Com o processamento de vídeo ligado, o leitor insere o conversor que
        // faltar — que é o caminho normal, porque quase nenhuma webcam entrega
        // NV12 direto. Se ainda assim recusar, tenta sem fixar tamanho e taxa:
        // resolução errada é melhor do que câmera nenhuma, e o escalador
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

        // O tamanho real é o que o leitor diz depois de negociar, e não o que
        // pedimos: ele pode ter aceitado outro.
        let current = reader
            .GetCurrentMediaType(stream)
            .map_err(|e| CameraError::from_hresult(&e, "lendo o formato negociado"))?;
        Ok(frame_size(&current).unwrap_or(chosen.size))
    }
}

/// A running Media Foundation capture.
pub(super) struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    pub(super) fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Spawns the reading loop. The device is proven open by `probe` first.
pub(super) fn start(
    device_id: &str,
    ceiling: Size,
    fps: u32,
    frames: Frames,
    on_lost: OnLost,
) -> Result<Capture, CameraError> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let device = device_id.to_owned();

    let thread = std::thread::Builder::new()
        .name("ldktela-camera".into())
        .spawn(move || {
            if let Err(error) = run(&device, ceiling, fps, frames, &thread_stop) {
                eprintln!("camera: captura interrompida: {error}");
                on_lost(error.to_string());
            }
        })
        .map_err(|_| CameraError::Platform("nao consegui criar a thread".into()))?;

    Ok(Capture {
        stop,
        thread: Some(thread),
    })
}

fn run(
    device_id: &str,
    ceiling: Size,
    fps: u32,
    mut frames: Frames,
    stop: &AtomicBool,
) -> Result<(), CameraError> {
    let _session = Session::new()?;
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
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

    while !stop.load(Ordering::Relaxed) {
        let mut flags = 0u32;
        let mut sample: Option<IMFSample> = None;
        unsafe {
            reader
                .ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
                .map_err(|e| CameraError::from_hresult(&e, "lendo um quadro"))?;
        }

        if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
            return Err(CameraError::Gone);
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            // A câmera foi desconectada. Não é o mesmo que parar: quem
            // compartilha precisa saber.
            return Err(CameraError::Gone);
        }
        // Sem amostra é rotina: o leitor devolve vazio quando o formato mudou ou
        // quando o dispositivo ainda está acordando.
        let Some(sample) = sample else {
            continue;
        };

        frames.deliver(captured, |nv12| copy_nv12(&sample, captured, nv12));
    }

    unsafe {
        let _ = source.Shutdown();
    }
    Ok(())
}

/// Copies one NV12 sample into `dst`, honouring both pitches.
fn copy_nv12(sample: &IMFSample, size: Size, dst: &mut NV12Buffer) -> bool {
    let (stride_y, stride_uv) = dst.strides();
    let (stride_y, stride_uv) = (stride_y as usize, stride_uv as usize);

    unsafe {
        let Ok(buffer) = sample.ConvertToContiguousBuffer() else {
            return false;
        };

        // `IMF2DBuffer` é o caminho certo quando existe: ele conhece o pitch. O
        // outro assume linhas coladas, que é o que um buffer contíguo entrega.
        if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
            let mut scanline = std::ptr::null_mut();
            let mut pitch = 0i32;
            if two_d.Lock2D(&mut scanline, &mut pitch).is_err() {
                return false;
            }
            let ok = pitch > 0 && {
                let pitch = pitch as usize;
                let rows = size.height as usize;
                let total = pitch * rows + pitch * (rows / 2);
                let source = std::slice::from_raw_parts(scanline, total);
                let (dst_y, dst_uv) = dst.data_mut();
                nv12_into(source, pitch, size, dst_y, stride_y, dst_uv, stride_uv)
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
            let (dst_y, dst_uv) = dst.data_mut();
            nv12_into(source, pitch, size, dst_y, stride_y, dst_uv, stride_uv)
        };
        let _ = buffer.Unlock();
        ok
    }
}
