//! The DirectShow capture path (ADR-0039).
//!
//! Exists for the cameras Media Foundation enumerates and will not open —
//! DroidCam, OBS Virtual Camera, Iriun and the rest of the virtual camera family
//! this product's audience actually uses.
//!
//! DirectShow has no source reader. A camera is a **graph**: the device's filter
//! connected to a filter that receives frames, with the graph manager running
//! both. That receiving filter is ours (ADR-0039, decision 6), which is why most
//! of this file is COM plumbing: `IBaseFilter`, its one `IPin`, and the
//! `IMemInputPin` the device pushes samples into.
//!
//! **Push, not pull** (decision 9). Nothing here loops over frames: the device's
//! own streaming thread calls `Receive`, and the thread we spawn only builds the
//! graph, waits, and tears it down. It is also where COM lives — an MTA object
//! is only valid while some thread in the process still holds the apartment
//! open, so the thread outlives the graph on purpose.
//!
//! Reference counting, which is where a filter like this goes wrong:
//! - the filter owns its pin (strong),
//! - the pin points back at the filter **without** a reference, and at the graph
//!   likewise, because either one closed into a cycle would keep the camera open
//!   forever,
//! - the peer pin is held strongly while connected, and the graph breaks that by
//!   disconnecting during `RemoveFilter`, which is why teardown calls it.

use std::ffi::c_void;
use std::mem::{size_of, ManuallyDrop};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use windows::core::{implement, ComObject, Interface, Ref, BOOL, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{E_NOTIMPL, E_OUTOFMEMORY, E_UNEXPECTED, S_FALSE, S_OK};
use windows::Win32::Media::DirectShow::{
    IAMStreamConfig, IBaseFilter, IBaseFilter_Impl, IEnumMediaTypes, IEnumMediaTypes_Impl,
    IEnumPins, IEnumPins_Impl, IFilterGraph, IGraphBuilder, IMediaControl, IMediaEvent,
    IMediaFilter_Impl, IMediaSample, IMemAllocator, IMemInputPin, IMemInputPin_Impl, IPin,
    IPin_Impl, State_Paused, State_Running, State_Stopped, ALLOCATOR_PROPERTIES, EC_DEVICE_LOST,
    EC_ERRORABORT, FILTER_INFO, FILTER_STATE, PINDIR_INPUT, PINDIR_OUTPUT, PIN_DIRECTION, PIN_INFO,
    VFW_E_NOT_CONNECTED, VFW_E_NO_ALLOCATOR, VFW_E_TYPE_NOT_ACCEPTED, VIDEO_STREAM_CONFIG_CAPS,
};
use windows::Win32::Media::IReferenceClock;
use windows::Win32::Media::MediaFoundation::{
    FORMAT_VideoInfo, MEDIATYPE_Video, AM_MEDIA_TYPE, VIDEOINFOHEADER,
};
use windows::Win32::System::Com::StructuredStorage::IPropertyBag;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoTaskMemFree, CoUninitialize, IMoniker,
    IPersist_Impl, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::VariantClear;

use super::convert::{Converter, Pixels, PREFERENCE};
use super::{choose_format, CameraError, Format, Frames, OnLost};
use crate::capture::Size;

/// `CLSID_SystemDeviceEnum`, `CLSID_VideoInputDeviceCategory`, `CLSID_FilterGraph`.
///
/// Written out because they are `DEFINE_GUID` in the headers and never made it
/// into the metadata the bindings are generated from.
const CLSID_SYSTEM_DEVICE_ENUM: GUID = GUID::from_u128(0x62be5d10_60eb_11d0_bd3b_00a0c911ce86);
const CLSID_VIDEO_INPUT_CATEGORY: GUID = GUID::from_u128(0x860bb310_5d01_11d0_bd3b_00a0c911ce86);
const CLSID_FILTER_GRAPH: GUID = GUID::from_u128(0xe436ebb3_524f_11ce_9f53_0020af0ba770);

/// The name of our pin, in `FindPin` and `QueryId`.
const PIN_NAME: &str = "In";

/// A mutex a media thread must never panic on.
///
/// Poisoning means another thread panicked while holding it; the frame data is
/// still just bytes, and taking the lock anyway beats poisoning the camera too.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// COM on this thread, for as long as this value lives.
struct ComSession;

impl ComSession {
    fn new() -> Self {
        unsafe {
            // Já inicializado por outra parte do processo é normal, e o
            // `HRESULT` de aviso não é falha.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        Self
    }
}

impl Drop for ComSession {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

/// One camera as DirectShow names it.
pub(super) struct Device {
    /// The moniker display name, which is also the handle `start` reopens it by.
    pub(super) moniker: String,
    pub(super) name: String,
    /// The device path, when this is a physical device. Virtual cameras
    /// registered as software have none.
    pub(super) path: Option<String>,
}

/// Walks the video input category, calling `visit` until it says stop.
unsafe fn for_each_moniker(
    mut visit: impl FnMut(&IMoniker, &str) -> bool,
) -> Result<(), windows::core::Error> {
    let enumerator: windows::Win32::Media::DirectShow::ICreateDevEnum =
        unsafe { CoCreateInstance(&CLSID_SYSTEM_DEVICE_ENUM, None, CLSCTX_INPROC_SERVER) }?;

    let mut monikers = None;
    // Devolve S_FALSE, e nenhum enumerador, quando a categoria está vazia —
    // que é o caso de uma máquina sem câmera nenhuma.
    unsafe { enumerator.CreateClassEnumerator(&CLSID_VIDEO_INPUT_CATEGORY, &mut monikers, 0) }?;
    let Some(monikers) = monikers else {
        return Ok(());
    };

    loop {
        let mut slot: [Option<IMoniker>; 1] = [None];
        let mut fetched = 0u32;
        if unsafe { monikers.Next(&mut slot, Some(&mut fetched)) } != S_OK || fetched == 0 {
            return Ok(());
        }
        let Some(moniker) = slot[0].take() else {
            return Ok(());
        };
        let Ok(display) = (unsafe { moniker.GetDisplayName(None, None) }) else {
            continue;
        };
        let name = unsafe { display.to_string() };
        unsafe { CoTaskMemFree(Some(display.as_ptr().cast())) };
        let Ok(name) = name else { continue };
        if !visit(&moniker, &name) {
            return Ok(());
        }
    }
}

/// Reads one string out of a moniker's property bag.
unsafe fn property(moniker: &IMoniker, key: PCWSTR) -> Option<String> {
    use windows::Win32::System::Variant::{VARIANT, VT_BSTR};

    let bag: IPropertyBag = unsafe { moniker.BindToStorage(None, None) }.ok()?;
    let mut variant = VARIANT::default();
    let read = unsafe { bag.Read(key, &mut variant, None) };
    let value = if read.is_ok() {
        let inner = unsafe { &variant.Anonymous.Anonymous };
        if inner.vt == VT_BSTR {
            Some(unsafe { inner.Anonymous.bstrVal.to_string() })
        } else {
            None
        }
    } else {
        None
    };
    // `VARIANT` não se limpa sozinho nestes bindings, e uma BSTR vazada por
    // câmera por abertura de menu é um vazamento que ninguém iria notar até
    // ficar grande.
    let _ = unsafe { VariantClear(&mut variant) };
    value
}

/// The device path hidden in a pnp moniker name, if this is one.
fn path_of(moniker_name: &str) -> Option<String> {
    moniker_name.strip_prefix("@device:pnp:").map(str::to_owned)
}

/// Lists the cameras DirectShow knows.
pub(super) fn list() -> Vec<Device> {
    let _com = ComSession::new();
    let mut devices = Vec::new();
    let collect = |moniker: &IMoniker, display: &str| {
        let name = unsafe { property(moniker, windows::core::w!("FriendlyName")) };
        devices.push(Device {
            path: path_of(display)
                .or_else(|| unsafe { property(moniker, windows::core::w!("DevicePath")) }),
            moniker: display.to_owned(),
            name: name.unwrap_or_else(|| "Câmera".to_owned()),
        });
        true
    };
    let walked = unsafe { for_each_moniker(collect) };
    if let Err(error) = walked {
        eprintln!("camera: o DirectShow nao enumerou: {error}");
    }
    devices
}

/// The DirectShow moniker for a camera Media Foundation named but would not open.
///
/// Matches on the hardware instance first, then on a friendly name that belongs
/// to exactly one device (ADR-0039, decision 3). An ambiguous name is no match
/// at all: opening the wrong camera is worse than reporting the first failure.
pub(super) fn counterpart(mf_id: &str) -> Option<String> {
    let devices = list();

    if let Some(key) = super::device_key(mf_id) {
        let found = devices.iter().find(|device| {
            device
                .path
                .as_deref()
                .and_then(super::device_key)
                .is_some_and(|theirs| theirs == key)
        });
        if let Some(found) = found {
            return Some(found.moniker.clone());
        }
    }

    let name = super::mf::name_of(mf_id)?;
    let mut matching = devices.iter().filter(|device| device.name == name);
    let first = matching.next()?;
    matching.next().is_none().then(|| first.moniker.clone())
}

/// A video format as our sink understands it.
#[derive(Clone)]
struct Shape {
    kind: Pixels,
    size: Size,
    subtype: GUID,
    /// The `VIDEOINFOHEADER` exactly as the device gave it, kept so
    /// `ConnectionMediaType` can answer with the truth rather than a rebuild.
    format: Vec<u8>,
}

/// Reads a media type, if it is one we can both understand and convert.
unsafe fn shape_of(pmt: *const AM_MEDIA_TYPE) -> Option<Shape> {
    let media = unsafe { pmt.as_ref() }?;
    if media.majortype != MEDIATYPE_Video || media.formattype != FORMAT_VideoInfo {
        return None;
    }
    if media.pbFormat.is_null() || (media.cbFormat as usize) < size_of::<VIDEOINFOHEADER>() {
        return None;
    }
    let kind = Pixels::from_subtype(&media.subtype)?;
    let header = unsafe { &*media.pbFormat.cast::<VIDEOINFOHEADER>() };
    let width = header.bmiHeader.biWidth;
    // Altura negativa é a forma de o bitmap dizer "de cima para baixo"; o
    // sentido já está decidido pelo formato, então aqui só o tamanho importa.
    let height = header.bmiHeader.biHeight.abs();
    if width < 2 || height < 2 || width % 2 != 0 || height % 2 != 0 {
        return None;
    }
    Some(Shape {
        kind,
        size: Size {
            width: width as u32,
            height: height as u32,
        },
        subtype: media.subtype,
        format: unsafe {
            std::slice::from_raw_parts(media.pbFormat, media.cbFormat as usize).to_vec()
        },
    })
}

/// Releases an `AM_MEDIA_TYPE` the way DirectShow allocated it.
unsafe fn free_media_type(pmt: *mut AM_MEDIA_TYPE) {
    if pmt.is_null() {
        return;
    }
    let media = unsafe { &mut *pmt };
    if !media.pbFormat.is_null() && media.cbFormat > 0 {
        unsafe { CoTaskMemFree(Some(media.pbFormat.cast())) };
        media.pbFormat = std::ptr::null_mut();
        media.cbFormat = 0;
    }
    unsafe { ManuallyDrop::drop(&mut media.pUnk) };
    unsafe { CoTaskMemFree(Some(pmt.cast())) };
}

/// Writes a copy of `shape` into a caller-owned `AM_MEDIA_TYPE`.
unsafe fn write_media_type(shape: &Shape, into: *mut AM_MEDIA_TYPE) -> Result<(), HRESULT> {
    if into.is_null() {
        return Err(E_UNEXPECTED);
    }
    let block = unsafe { CoTaskMemAlloc(shape.format.len()) };
    if block.is_null() {
        return Err(E_OUTOFMEMORY);
    }
    unsafe {
        std::ptr::copy_nonoverlapping(
            shape.format.as_ptr(),
            block.cast::<u8>(),
            shape.format.len(),
        );
        std::ptr::write(
            into,
            AM_MEDIA_TYPE {
                majortype: MEDIATYPE_Video,
                subtype: shape.subtype,
                bFixedSizeSamples: BOOL(1),
                bTemporalCompression: BOOL(0),
                lSampleSize: shape.kind.frame_bytes(shape.size) as u32,
                formattype: FORMAT_VideoInfo,
                pUnk: ManuallyDrop::new(None),
                cbFormat: shape.format.len() as u32,
                pbFormat: block.cast(),
            },
        );
    }
    Ok(())
}

/// Picks the format and tells the device to use it.
///
/// Unlike the Media Foundation path, which asks for NV12 and lets Windows insert
/// a converter, this negotiates a format the device **already emits** and
/// converts in `convert.rs` (ADR-0039, decision 7).
unsafe fn negotiate(pin: &IPin, ceiling: Size, fps: u32) -> Result<Shape, CameraError> {
    let config: IAMStreamConfig = pin.cast().map_err(|_| CameraError::NoUsableFormat)?;
    let (mut count, mut caps_size) = (0i32, 0i32);
    unsafe { config.GetNumberOfCapabilities(&mut count, &mut caps_size) }
        .map_err(|e| CameraError::from_hresult(&e, "lendo os formatos"))?;
    if count <= 0 || caps_size as usize != size_of::<VIDEO_STREAM_CONFIG_CAPS>() {
        return Err(CameraError::NoUsableFormat);
    }

    let mut caps = vec![0u8; caps_size as usize];
    let mut candidates: Vec<(i32, Format, Pixels)> = Vec::new();
    let mut saw_video = false;

    for index in 0..count {
        let mut pmt: *mut AM_MEDIA_TYPE = std::ptr::null_mut();
        if unsafe { config.GetStreamCaps(index, &mut pmt, caps.as_mut_ptr()) }.is_err() {
            continue;
        }
        if unsafe { pmt.as_ref() }.is_some_and(|m| m.majortype == MEDIATYPE_Video) {
            saw_video = true;
        }
        if let Some(shape) = unsafe { shape_of(pmt) } {
            let caps = unsafe { &*caps.as_ptr().cast::<VIDEO_STREAM_CONFIG_CAPS>() };
            candidates.push((
                index,
                Format {
                    size: shape.size,
                    fps: achievable_fps(caps, fps),
                },
                shape.kind,
            ));
        }
        unsafe { free_media_type(pmt) };
    }

    if candidates.is_empty() {
        // Toda câmera que emite MJPEG emite também algo cru em alguma
        // resolução; quando não emite, dizemos isso em vez de deixar o
        // DirectShow montar um decodificador por conta (ADR-0039, decisão 8).
        return Err(if saw_video {
            CameraError::OnlyCompressed
        } else {
            CameraError::NoUsableFormat
        });
    }

    let offered: Vec<Format> = candidates.iter().map(|(_, format, _)| *format).collect();
    let chosen = choose_format(&offered, ceiling, fps).ok_or(CameraError::NoUsableFormat)?;
    // Entre os formatos do mesmo tamanho e taxa, o layout mais barato de
    // converter ganha.
    let (index, format, _) = candidates
        .iter()
        .filter(|(_, f, _)| f.size == chosen.size && f.fps == chosen.fps)
        .min_by_key(|(_, _, kind)| {
            PREFERENCE
                .iter()
                .position(|preferred| preferred == kind)
                .unwrap_or(usize::MAX)
        })
        .ok_or(CameraError::NoUsableFormat)?;
    let (index, wanted_fps) = (*index, format.fps);

    let mut pmt: *mut AM_MEDIA_TYPE = std::ptr::null_mut();
    unsafe { config.GetStreamCaps(index, &mut pmt, caps.as_mut_ptr()) }
        .map_err(|e| CameraError::from_hresult(&e, "relendo o formato escolhido"))?;
    let shape = unsafe { shape_of(pmt) };
    if wanted_fps > 0 {
        // A taxa vive no cabeçalho, em unidades de 100 ns por quadro.
        if let Some(media) = unsafe { pmt.as_mut() } {
            if !media.pbFormat.is_null() {
                let header = unsafe { &mut *media.pbFormat.cast::<VIDEOINFOHEADER>() };
                header.AvgTimePerFrame = 10_000_000 / i64::from(wanted_fps);
            }
        }
    }
    let applied = unsafe { config.SetFormat(pmt) };
    // Relê a forma depois de `SetFormat`, porque a taxa que escrevemos faz
    // parte dela.
    let shape = unsafe { shape_of(pmt) }.or(shape);
    unsafe { free_media_type(pmt) };
    applied.map_err(|e| CameraError::from_hresult(&e, "fixando o formato"))?;
    shape.ok_or(CameraError::NoUsableFormat)
}

/// The frame rate this capability can actually deliver, closest to `wanted`.
fn achievable_fps(caps: &VIDEO_STREAM_CONFIG_CAPS, wanted: u32) -> u32 {
    let rate = |interval: i64| -> u32 {
        if interval > 0 {
            (10_000_000 / interval) as u32
        } else {
            0
        }
    };
    // Intervalo menor é taxa maior, então os extremos trocam de lado.
    let fastest = rate(caps.MinFrameInterval);
    let slowest = rate(caps.MaxFrameInterval);
    if fastest == 0 || slowest == 0 || slowest > fastest {
        return wanted;
    }
    wanted.clamp(slowest, fastest)
}

/// The output pin frames come out of.
///
/// The first output pin that can configure its own format. Capture filters put
/// the capture pin first and virtual cameras expose exactly one, so looking the
/// category up through `IKsPropertySet` would buy nothing here.
unsafe fn output_pin(filter: &IBaseFilter) -> Result<IPin, CameraError> {
    let pins = unsafe { filter.EnumPins() }
        .map_err(|e| CameraError::from_hresult(&e, "listando os pinos da camera"))?;
    let mut fallback: Option<IPin> = None;

    loop {
        let mut slot: [Option<IPin>; 1] = [None];
        let mut fetched = 0u32;
        if unsafe { pins.Next(&mut slot, Some(&mut fetched)) } != S_OK || fetched == 0 {
            break;
        }
        let Some(pin) = slot[0].take() else { break };
        if unsafe { pin.QueryDirection() }.ok() != Some(PINDIR_OUTPUT) {
            continue;
        }
        if pin.cast::<IAMStreamConfig>().is_ok() {
            return Ok(pin);
        }
        fallback.get_or_insert(pin);
    }
    fallback.ok_or(CameraError::NoUsableFormat)
}

/// A built, running graph. Everything here is created and released on one thread.
struct Graph {
    graph: IGraphBuilder,
    control: IMediaControl,
    events: IMediaEvent,
    capture: IBaseFilter,
    sink: IBaseFilter,
    /// What we negotiated, in words, to put in the failure the person sees.
    ///
    /// A camera that dies a second after starting says nothing by itself; the
    /// same failure with `YUY2 1280x720` attached says which negotiation to go
    /// look at, and whether two machines chose differently.
    negotiated: String,
    /// Declarado por último de propósito: os campos são largados na ordem em que
    /// aparecem, e nenhum objeto COM pode sobreviver ao apartamento.
    _com: ComSession,
}

impl Graph {
    fn teardown(self) {
        unsafe {
            let _ = self.control.Stop();
            // `RemoveFilter` desconecta os pinos, que é o que quebra a
            // referência do pino da câmera para o nosso.
            let _ = self.graph.RemoveFilter(&self.sink);
            let _ = self.graph.RemoveFilter(&self.capture);
        }
    }
}

fn build(
    moniker_name: &str,
    ceiling: Size,
    fps: u32,
    frames: Frames,
) -> Result<Graph, CameraError> {
    let com = ComSession::new();
    unsafe {
        let mut bound: Option<IBaseFilter> = None;
        let walked = for_each_moniker(|moniker, display| {
            if display != moniker_name {
                return true;
            }
            bound = moniker.BindToObject::<Option<&windows::Win32::System::Com::IBindCtx>, Option<&IMoniker>, IBaseFilter>(None, None).ok();
            false
        });
        if let Err(error) = walked {
            return Err(CameraError::from_hresult(&error, "procurando a camera"));
        }
        let capture = bound.ok_or(CameraError::Gone)?;

        let graph: IGraphBuilder =
            CoCreateInstance(&CLSID_FILTER_GRAPH, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| CameraError::from_hresult(&e, "criando o grafo"))?;
        graph
            .AddFilter(&capture, windows::core::w!("Camera"))
            .map_err(|e| CameraError::from_hresult(&e, "ligando a camera ao grafo"))?;

        let source = output_pin(&capture)?;
        let shape = negotiate(&source, ceiling, fps)?;
        let negotiated = format!(
            "{:?} {}x{}",
            shape.kind, shape.size.width, shape.size.height
        );
        eprintln!("camera: DirectShow negociou {negotiated}");

        let (sink, sink_pin) = SinkFilter::build(frames, shape);
        graph
            .AddFilter(&sink, windows::core::w!("ldktela"))
            .map_err(|e| CameraError::from_hresult(&e, "ligando o destino ao grafo"))?;
        // `ConnectDirect`, e não `Connect`: o conector "inteligente" do
        // DirectShow monta filtros de terceiros dentro do nosso processo, e o
        // formato já foi negociado (ADR-0039, decisão 8).
        graph
            .ConnectDirect(&source, &sink_pin, None)
            .map_err(|e| CameraError::from_hresult(&e, "conectando a camera ao destino"))?;

        let control: IMediaControl = graph
            .cast()
            .map_err(|e| CameraError::from_hresult(&e, "controlando o grafo"))?;
        let events: IMediaEvent = graph
            .cast()
            .map_err(|e| CameraError::from_hresult(&e, "ouvindo o grafo"))?;
        control
            .Run()
            .map_err(|e| CameraError::from_hresult(&e, "iniciando a camera"))?;

        Ok(Graph {
            graph,
            control,
            events,
            capture,
            sink,
            negotiated,
            _com: com,
        })
    }
}

/// Waits for the stop request, watching for the device going away.
///
/// Returns why the camera was lost, or `None` when it was simply stopped.
fn pump(graph: &Graph, stop: &AtomicBool) -> Option<String> {
    while !stop.load(Ordering::Relaxed) {
        let (mut code, mut first, mut second) = (0i32, 0isize, 0isize);
        // 200 ms: curto o bastante para o pedido de parada não ficar pendurado,
        // longo o bastante para a espera não custar nada.
        if unsafe {
            graph
                .events
                .GetEvent(&mut code, &mut first, &mut second, 200)
        }
        .is_err()
        {
            continue;
        }
        let _ = unsafe { graph.events.FreeEventParams(code, first, second) };
        match code as u32 {
            // `second == 1` é a câmera **voltando**, e não indo embora.
            EC_DEVICE_LOST if second != 1 => return Some("a camera foi desconectada".to_owned()),
            EC_ERRORABORT => {
                return Some(format!(
                    "o Windows interrompeu a camera ({first:#010x}, em {})",
                    graph.negotiated
                ))
            }
            _ => {}
        }
    }
    None
}

/// A running DirectShow capture.
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

/// Builds the graph on its own thread and waits for it to be running.
///
/// The build happens there, and not here, because the graph may only be touched
/// from a thread that keeps COM initialised — but the caller still learns
/// whether the camera opened, because the answer comes back over a channel
/// before this returns.
pub(super) fn start(
    moniker_name: &str,
    ceiling: Size,
    fps: u32,
    frames: Frames,
    on_lost: OnLost,
) -> Result<Capture, CameraError> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let moniker = moniker_name.to_owned();
    let (ready, opened) = mpsc::channel();

    let thread = std::thread::Builder::new()
        .name("ldktela-camera-dshow".into())
        .spawn(move || match build(&moniker, ceiling, fps, frames) {
            Err(error) => {
                let _ = ready.send(Err(error));
            }
            Ok(graph) => {
                let _ = ready.send(Ok(()));
                let lost = pump(&graph, &thread_stop);
                graph.teardown();
                if let Some(reason) = lost {
                    on_lost(reason);
                }
            }
        })
        .map_err(|_| CameraError::Platform("nao consegui criar a thread".into()))?;

    match opened.recv() {
        Ok(Ok(())) => Ok(Capture {
            stop,
            thread: Some(thread),
        }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => Err(CameraError::Platform(
            "a thread da camera terminou antes de abrir".into(),
        )),
    }
}

// ---------------------------------------------------------------------------
// The sink filter
// ---------------------------------------------------------------------------

/// Our end of the graph: one filter, one input pin, no output.
#[implement(IBaseFilter)]
struct SinkFilter {
    pin: IPin,
    state: Mutex<FILTER_STATE>,
    /// The graph, **without** a reference. `JoinFilterGraph` is documented to
    /// store it weakly, and holding it strongly would mean the graph keeps the
    /// filter which keeps the graph.
    graph: AtomicPtr<c_void>,
    clock: Mutex<Option<IReferenceClock>>,
}

impl SinkFilter {
    fn build(frames: Frames, shape: Shape) -> (IBaseFilter, IPin) {
        let pin = ComObject::new(SinkPin {
            owner: AtomicPtr::new(std::ptr::null_mut()),
            peer: Mutex::new(None),
            sink: Mutex::new(Sink {
                converter: Converter::new(shape.kind),
                shape,
                frames,
            }),
        });
        let pin_interface: IPin = pin.to_interface();

        let filter: IBaseFilter = ComObject::new(SinkFilter {
            pin: pin_interface.clone(),
            state: Mutex::new(State_Stopped),
            graph: AtomicPtr::new(std::ptr::null_mut()),
            clock: Mutex::new(None),
        })
        .to_interface();

        // O elo de volta, sem referência: o filtro é dono do pino, e o pino
        // vive dentro dele.
        pin.owner.store(filter.as_raw(), Ordering::Release);
        (filter, pin_interface)
    }
}

impl IPersist_Impl for SinkFilter_Impl {
    fn GetClassID(&self) -> windows::core::Result<GUID> {
        // Nosso, e nunca registrado: o filtro só existe dentro deste processo.
        Ok(GUID::from_u128(0x6c0c1b4e_9f3a_4d21_8a7e_1d5b2f7c4a90))
    }
}

impl IMediaFilter_Impl for SinkFilter_Impl {
    fn Stop(&self) -> windows::core::Result<()> {
        *lock(&self.state) = State_Stopped;
        Ok(())
    }

    fn Pause(&self) -> windows::core::Result<()> {
        *lock(&self.state) = State_Paused;
        Ok(())
    }

    fn Run(&self, _start: i64) -> windows::core::Result<()> {
        *lock(&self.state) = State_Running;
        Ok(())
    }

    fn GetState(&self, _timeout: u32) -> windows::core::Result<FILTER_STATE> {
        Ok(*lock(&self.state))
    }

    fn SetSyncSource(&self, clock: Ref<'_, IReferenceClock>) -> windows::core::Result<()> {
        *lock(&self.clock) = clock.ok().ok().cloned();
        Ok(())
    }

    fn GetSyncSource(&self) -> windows::core::Result<IReferenceClock> {
        lock(&self.clock)
            .clone()
            .ok_or_else(|| windows::core::Error::from_hresult(E_UNEXPECTED))
    }
}

impl IBaseFilter_Impl for SinkFilter_Impl {
    fn EnumPins(&self) -> windows::core::Result<IEnumPins> {
        Ok(ComObject::new(PinList {
            pin: self.pin.clone(),
            at: Mutex::new(0),
        })
        .to_interface())
    }

    fn FindPin(&self, id: &PCWSTR) -> windows::core::Result<IPin> {
        let wanted = unsafe { id.to_string() }.unwrap_or_default();
        if wanted == PIN_NAME {
            Ok(self.pin.clone())
        } else {
            Err(windows::core::Error::from_hresult(VFW_E_NOT_CONNECTED))
        }
    }

    fn QueryFilterInfo(&self, info: *mut FILTER_INFO) -> windows::core::Result<()> {
        let info = unsafe { info.as_mut() }
            .ok_or_else(|| windows::core::Error::from_hresult(E_UNEXPECTED))?;
        write_name(&mut info.achName, "ldktela");

        // O grafo devolvido é do chamador, que o libera: por isso a referência
        // nova, que é o único lugar em que este ponteiro fraco vira forte.
        let raw = self.graph.load(Ordering::Acquire);
        let graph = if raw.is_null() {
            None
        } else {
            let borrowed = ManuallyDrop::new(unsafe { IFilterGraph::from_raw(raw) });
            Some((*borrowed).clone())
        };
        unsafe { std::ptr::write(&mut info.pGraph, ManuallyDrop::new(graph)) };
        Ok(())
    }

    fn JoinFilterGraph(
        &self,
        graph: Ref<'_, IFilterGraph>,
        _name: &PCWSTR,
    ) -> windows::core::Result<()> {
        let raw = match graph.ok() {
            Ok(graph) => graph.as_raw(),
            Err(_) => std::ptr::null_mut(),
        };
        self.graph.store(raw, Ordering::Release);
        Ok(())
    }

    fn QueryVendorInfo(&self) -> windows::core::Result<PWSTR> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }
}

fn write_name(into: &mut [u16; 128], name: &str) {
    let wide: Vec<u16> = name.encode_utf16().take(127).collect();
    into[..wide.len()].copy_from_slice(&wide);
    into[wide.len()] = 0;
}

/// The one-pin enumerator `EnumPins` hands back.
#[implement(IEnumPins)]
struct PinList {
    pin: IPin,
    at: Mutex<u32>,
}

impl IEnumPins_Impl for PinList_Impl {
    fn Next(&self, count: u32, into: *mut Option<IPin>, fetched: *mut u32) -> HRESULT {
        let mut at = lock(&self.at);
        let mut written = 0u32;
        if count > 0 && *at == 0 && !into.is_null() {
            unsafe { std::ptr::write(into, Some(self.pin.clone())) };
            *at = 1;
            written = 1;
        }
        if !fetched.is_null() {
            unsafe { *fetched = written };
        }
        if written == count {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn Skip(&self, count: u32) -> windows::core::Result<()> {
        let mut at = lock(&self.at);
        *at = at.saturating_add(count);
        if *at > 1 {
            return Err(windows::core::Error::from_hresult(S_FALSE));
        }
        Ok(())
    }

    fn Reset(&self) -> windows::core::Result<()> {
        *lock(&self.at) = 0;
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IEnumPins> {
        Ok(ComObject::new(PinList {
            pin: self.pin.clone(),
            at: Mutex::new(*lock(&self.at)),
        })
        .to_interface())
    }
}

/// An empty media type enumerator.
///
/// An input pin proposes nothing: the connection is made with the type the
/// output pin offers. Returning an empty enumerator is what the DirectShow base
/// classes do, and an error here would be read as a broken pin.
#[implement(IEnumMediaTypes)]
struct NoTypes;

impl IEnumMediaTypes_Impl for NoTypes_Impl {
    fn Next(&self, _count: u32, _into: *mut *mut AM_MEDIA_TYPE, fetched: *mut u32) -> HRESULT {
        if !fetched.is_null() {
            unsafe { *fetched = 0 };
        }
        S_FALSE
    }

    fn Skip(&self, _count: u32) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(S_FALSE))
    }

    fn Reset(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IEnumMediaTypes> {
        Ok(ComObject::new(NoTypes).to_interface())
    }
}

/// What a received frame is turned into, all behind one lock.
struct Sink {
    frames: Frames,
    converter: Converter,
    shape: Shape,
}

/// The input pin frames arrive on.
#[implement(IPin, IMemInputPin)]
struct SinkPin {
    /// The filter that owns this pin, **without** a reference: the filter holds
    /// the pin, so a strong pointer back would be a cycle nothing breaks.
    owner: AtomicPtr<c_void>,
    peer: Mutex<Option<IPin>>,
    sink: Mutex<Sink>,
}

impl SinkPin_Impl {
    /// Where every DirectShow frame arrives, on the device's streaming thread.
    fn take(&self, sample: &IMediaSample) -> windows::core::Result<()> {
        let mut sink = lock(&self.sink);

        // Formato trocado no meio do fluxo: raro, e silencioso quando ignorado —
        // a imagem passaria a ser lida com o tamanho errado.
        if let Ok(changed) = unsafe { sample.GetMediaType() } {
            if !changed.is_null() {
                if let Some(shape) = unsafe { shape_of(changed) } {
                    if sink.converter.kind() != shape.kind {
                        sink.converter = Converter::new(shape.kind);
                    }
                    sink.shape = shape;
                }
                unsafe { free_media_type(changed) };
            }
        }

        let data = unsafe { sample.GetPointer() }?;
        let length = unsafe { sample.GetActualDataLength() };
        if data.is_null() || length <= 0 {
            return Ok(());
        }
        let needed = sink.shape.kind.frame_bytes(sink.shape.size);
        if (length as usize) < needed {
            return Ok(());
        }

        let Sink {
            frames,
            converter,
            shape,
        } = &mut *sink;
        let size = shape.size;
        // A amostra é emprestada pelo tempo desta chamada, então a conversão
        // acontece aqui dentro e nada do DirectShow escapa deste escopo.
        let bytes = unsafe { std::slice::from_raw_parts(data, needed) };
        frames.deliver(size, |destination| {
            converter.write(bytes, size, destination)
        });
        Ok(())
    }

    fn owner(&self) -> Option<IBaseFilter> {
        let raw = self.owner.load(Ordering::Acquire);
        if raw.is_null() {
            return None;
        }
        let borrowed = ManuallyDrop::new(unsafe { IBaseFilter::from_raw(raw) });
        Some((*borrowed).clone())
    }
}

impl IPin_Impl for SinkPin_Impl {
    fn Connect(
        &self,
        _peer: Ref<'_, IPin>,
        _media: *const AM_MEDIA_TYPE,
    ) -> windows::core::Result<()> {
        // Pino de entrada não inicia conexão.
        Err(windows::core::Error::from_hresult(E_UNEXPECTED))
    }

    fn ReceiveConnection(
        &self,
        peer: Ref<'_, IPin>,
        media: *const AM_MEDIA_TYPE,
    ) -> windows::core::Result<()> {
        let Some(shape) = (unsafe { shape_of(media) }) else {
            return Err(windows::core::Error::from_hresult(VFW_E_TYPE_NOT_ACCEPTED));
        };
        let mut sink = lock(&self.sink);
        if sink.converter.kind() != shape.kind {
            sink.converter = Converter::new(shape.kind);
        }
        sink.shape = shape;
        drop(sink);
        *lock(&self.peer) = peer.ok().ok().cloned();
        Ok(())
    }

    fn Disconnect(&self) -> windows::core::Result<()> {
        *lock(&self.peer) = None;
        Ok(())
    }

    fn ConnectedTo(&self) -> windows::core::Result<IPin> {
        lock(&self.peer)
            .clone()
            .ok_or_else(|| windows::core::Error::from_hresult(VFW_E_NOT_CONNECTED))
    }

    fn ConnectionMediaType(&self, into: *mut AM_MEDIA_TYPE) -> windows::core::Result<()> {
        if lock(&self.peer).is_none() {
            return Err(windows::core::Error::from_hresult(VFW_E_NOT_CONNECTED));
        }
        unsafe { write_media_type(&lock(&self.sink).shape, into) }
            .map_err(windows::core::Error::from_hresult)
    }

    fn QueryPinInfo(&self, info: *mut PIN_INFO) -> windows::core::Result<()> {
        let info = unsafe { info.as_mut() }
            .ok_or_else(|| windows::core::Error::from_hresult(E_UNEXPECTED))?;
        info.dir = PINDIR_INPUT;
        write_name(&mut info.achName, PIN_NAME);
        unsafe { std::ptr::write(&mut info.pFilter, ManuallyDrop::new(self.owner())) };
        Ok(())
    }

    fn QueryDirection(&self) -> windows::core::Result<PIN_DIRECTION> {
        Ok(PINDIR_INPUT)
    }

    fn QueryId(&self) -> windows::core::Result<PWSTR> {
        let wide: Vec<u16> = PIN_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let block = unsafe { CoTaskMemAlloc(wide.len() * size_of::<u16>()) }.cast::<u16>();
        if block.is_null() {
            return Err(windows::core::Error::from_hresult(E_OUTOFMEMORY));
        }
        unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), block, wide.len()) };
        Ok(PWSTR(block))
    }

    fn QueryAccept(&self, media: *const AM_MEDIA_TYPE) -> HRESULT {
        if unsafe { shape_of(media) }.is_some() {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn EnumMediaTypes(&self) -> windows::core::Result<IEnumMediaTypes> {
        Ok(ComObject::new(NoTypes).to_interface())
    }

    fn QueryInternalConnections(
        &self,
        _pins: windows::core::OutRef<'_, IPin>,
        _count: *mut u32,
    ) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }

    fn EndOfStream(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn BeginFlush(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn EndFlush(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn NewSegment(&self, _start: i64, _stop: i64, _rate: f64) -> windows::core::Result<()> {
        Ok(())
    }
}

impl IMemInputPin_Impl for SinkPin_Impl {
    fn GetAllocator(&self) -> windows::core::Result<IMemAllocator> {
        // Nenhum nosso: o alocador da câmera serve, e um nosso só acrescentaria
        // uma cópia.
        Err(windows::core::Error::from_hresult(VFW_E_NO_ALLOCATOR))
    }

    fn NotifyAllocator(
        &self,
        _allocator: Ref<'_, IMemAllocator>,
        _read_only: BOOL,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn GetAllocatorRequirements(&self) -> windows::core::Result<ALLOCATOR_PROPERTIES> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }

    fn Receive(&self, sample: Ref<'_, IMediaSample>) -> windows::core::Result<()> {
        self.take(sample.ok()?)
    }

    fn ReceiveMultiple(
        &self,
        samples: *const Option<IMediaSample>,
        count: i32,
    ) -> windows::core::Result<i32> {
        if samples.is_null() || count < 0 {
            return Err(windows::core::Error::from_hresult(E_UNEXPECTED));
        }
        let samples = unsafe { std::slice::from_raw_parts(samples, count as usize) };
        let mut done = 0i32;
        for sample in samples {
            let Some(sample) = sample.as_ref() else { break };
            self.take(sample)?;
            done += 1;
        }
        Ok(done)
    }

    fn ReceiveCanBlock(&self) -> windows::core::Result<()> {
        // `S_FALSE` é "não bloqueia": a conversão é síncrona e limitada.
        Err(windows::core::Error::from_hresult(S_FALSE))
    }
}
