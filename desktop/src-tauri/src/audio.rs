//! System audio capture that leaves the Discord out (RF-29, RF-30, ADR-0025).
//!
//! `AUDIOCLIENT_ACTIVATION_PARAMS` takes one process id and one mode. Pointed at
//! Discord with `EXCLUDE_TARGET_PROCESS_TREE`, it yields every sound the machine
//! is making **except** Discord's — which is the requirement, stated in the
//! Windows API's own words.
//!
//! Since publishing moved into this process (ADR-0026), the PCM goes straight
//! into the encoder's `NativeAudioSource`. It crosses no IPC and touches no
//! `AudioContext`, so the clock drift ADR-0014 called the roadmap's largest
//! uncertainty has nowhere to happen: capture and encode read the same clock.
//!
//! Human review zone (`CLAUDE.md` §10): OS media capture.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use serde::Serialize;
use windows::core::{implement, Interface, Ref};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eConsole, eRender, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoUninitialize, BLOB, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;

use crate::publisher::{CHANNELS, SAMPLE_RATE};

const BITS_PER_SAMPLE: u16 = 16;

/// 20 ms of slack in the shared buffer.
const BUFFER_DURATION_HNS: i64 = 200_000;
/// Only used by the whole-system path, which has no event to wait on.
const POLL: Duration = Duration::from_millis(10);

/// What the user is actually getting, so the interface can say so (RF-30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioMode {
    /// Process loopback, Discord's tree excluded. The intended path.
    ExcludingDiscord,
    /// No Discord running, or the platform refused process loopback. Everything
    /// the machine plays goes out, and the interface has to admit it.
    WholeSystem,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("o Windows recusou a captura de audio: {0}")]
    Windows(String),
    #[error("a captura de audio nao respondeu a tempo")]
    Timeout,
}

impl From<windows::core::Error> for AudioError {
    fn from(error: windows::core::Error) -> Self {
        AudioError::Windows(error.message())
    }
}

pub struct AudioCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Samples per channel handed to the encoder. Cheap, and the only way to
    /// tell "capturing silence" from "capturing nothing" — they look identical
    /// from outside and have completely different causes.
    delivered: Arc<AtomicU64>,
}

impl AudioCapture {
    pub fn delivered_samples(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed)
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts capturing and reports which of the two modes it got.
///
/// COM lives entirely inside the capture thread, so the caller's thread — a
/// Tauri worker — is never put into an apartment it did not ask for. The mode
/// comes back over a channel because it is only known after activation, and the
/// interface has to state it before the user starts talking over a stream that
/// is carrying everyone's voice back to them.
pub fn start(sink: NativeAudioSource) -> Result<(AudioCapture, AudioMode), AudioError> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let (ready_tx, ready_rx) = mpsc::channel::<Result<AudioMode, AudioError>>();

    // Fila curta de proposito: audio atrasado nao serve para nada, e crescer a
    // fila so troca falha audivel por memoria e atraso crescente.
    let (frames_tx, mut frames_rx) = tokio::sync::mpsc::channel::<Vec<i16>>(32);

    let delivered = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&delivered);
    tokio::spawn(async move {
        while let Some(samples) = frames_rx.recv().await {
            let samples_per_channel = (samples.len() / CHANNELS as usize) as u32;
            if samples_per_channel == 0 {
                continue;
            }
            let frame = AudioFrame {
                data: Cow::Owned(samples),
                sample_rate: SAMPLE_RATE,
                num_channels: CHANNELS,
                samples_per_channel,
            };
            if let Err(error) = sink.capture_frame(&frame).await {
                eprintln!("audio: quadro recusado pelo encoder: {error}");
                continue;
            }
            counted.fetch_add(u64::from(samples_per_channel), Ordering::Relaxed);
        }
    });

    let thread = std::thread::Builder::new()
        .name("ldktela-audio".into())
        .spawn(move || run(&thread_stop, &ready_tx, &frames_tx))
        .map_err(|_| AudioError::Timeout)?;

    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(mode)) => Ok((
            AudioCapture {
                stop,
                thread: Some(thread),
                delivered,
            },
            mode,
        )),
        Ok(Err(error)) => {
            stop.store(true, Ordering::Relaxed);
            let _ = thread.join();
            Err(error)
        }
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            Err(AudioError::Timeout)
        }
    }
}

fn run(
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<AudioMode, AudioError>>,
    frames: &tokio::sync::mpsc::Sender<Vec<i16>>,
) {
    // MTA: `ActivateAudioInterfaceAsync` completes on a pool thread, and an STA
    // would need a message pump for that to ever arrive.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if com.is_err() {
        let _ = ready.send(Err(AudioError::Windows(
            "nao consegui inicializar o COM".into(),
        )));
        return;
    }

    // Todo objeto COM vive dentro deste escopo. Soltar uma interface DEPOIS de
    // `CoUninitialize` e comportamento indefinido.
    {
        let opened = match open_client() {
            Ok(opened) => opened,
            Err(error) => {
                let _ = ready.send(Err(error));
                unsafe { CoUninitialize() };
                return;
            }
        };

        if let Err(error) = pump(&opened, stop, ready, frames) {
            eprintln!("audio: captura interrompida: {error}");
        }
    }

    unsafe { CoUninitialize() };
}

/// An opened capture, plus the event WASAPI signals when a packet is ready.
struct Opened {
    client: IAudioClient,
    /// `Some` when the client was initialised event-driven, which process
    /// loopback requires.
    tick: Option<HANDLE>,
    mode: AudioMode,
}

impl Drop for Opened {
    fn drop(&mut self) {
        if let Some(tick) = self.tick.take() {
            unsafe {
                let _ = CloseHandle(tick);
            }
        }
    }
}

/// Process loopback if Discord is running, whole-system loopback otherwise.
fn open_client() -> Result<Opened, AudioError> {
    let Some(pid) = discord_root_pid() else {
        return open_whole_system();
    };
    match open_process_loopback(pid) {
        Ok(opened) => Ok(opened),
        Err(error) => {
            // RF-30: cair para o sistema inteiro e melhor do que nao ter audio,
            // desde que a interface diga o que esta acontecendo.
            eprintln!(
                "audio: process loopback indisponivel ({error}), caindo para o sistema inteiro"
            );
            open_whole_system()
        }
    }
}

fn format() -> WAVEFORMATEX {
    let channels = CHANNELS as u16;
    let block_align = channels * BITS_PER_SAMPLE / 8;
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: channels,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * u32::from(block_align),
        nBlockAlign: block_align,
        wBitsPerSample: BITS_PER_SAMPLE,
        cbSize: 0,
    }
}

/// Signalled by WASAPI when the asynchronous activation finishes.
///
/// Signals through a Win32 event rather than a condition variable: this object
/// is built on our thread and called on a WASAPI pool thread, and an event
/// handle is the primitive both sides already agree on.
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion {
    /// `HANDLE` is a raw pointer and so not `Send`; the numeric value is, and
    /// the handle is only ever signalled — never closed — from the callback.
    done: usize,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        unsafe {
            let _ = SetEvent(HANDLE(self.done as *mut std::ffi::c_void));
        }
        Ok(())
    }
}

fn open_process_loopback(pid: u32) -> Result<Opened, AudioError> {
    // O blob vai em memoria do COM, e nao na pilha, porque o `mmdevapi` limpa o
    // PROPVARIANT que recebe — e limpar um VT_BLOB e `CoTaskMemFree(pBlobData)`.
    //
    // Isto custou uma sessao inteira de depuracao, e a amostra ApplicationLoopback
    // da propria Microsoft usa a pilha. Com o blob na pilha a captura funciona
    // perfeitamente, entrega os quadros certos, e destroi o heap do processo: a
    // morte vem depois, com STATUS_HEAP_CORRUPTION, em qualquer alocacao, longe
    // daqui. Esta memoria nao e devolvida por nos — quem a libera e o Windows.
    let params = unsafe {
        let ptr = CoTaskMemAlloc(std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>())
            .cast::<AUDIOCLIENT_ACTIVATION_PARAMS>();
        if ptr.is_null() {
            return Err(AudioError::Windows(
                "sem memoria para ativar a captura".into(),
            ));
        }
        ptr.write(AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
                },
            },
        });
        ptr
    };

    let mut activation = PROPVARIANT::default();
    unsafe {
        let inner = &mut activation.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob = BLOB {
            cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
            pBlobData: params.cast::<u8>(),
        };
    }

    let done = unsafe { CreateEventW(None, true, false, None) }?;
    let handler: IActivateAudioInterfaceCompletionHandler = Completion {
        done: done.0 as usize,
    }
    .into();

    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&activation),
            &handler,
        )
    }?;

    let waited = unsafe { WaitForSingleObject(done, 3_000) };
    unsafe {
        let _ = CloseHandle(done);
    }
    if waited != WAIT_OBJECT_0 {
        return Err(AudioError::Timeout);
    }

    let mut result = windows::core::HRESULT(0);
    let mut interface: Option<windows::core::IUnknown> = None;
    unsafe { operation.GetActivateResult(&mut result, &mut interface) }?;
    result.ok()?;
    let client: IAudioClient = interface
        .ok_or_else(|| AudioError::Windows("o Windows nao devolveu um IAudioClient".into()))?
        .cast()?;

    // Process loopback so aceita o modo dirigido por evento.
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            BUFFER_DURATION_HNS,
            0,
            &format(),
            None,
        )
    }?;
    let tick = unsafe { CreateEventW(None, false, false, None) }?;
    unsafe { client.SetEventHandle(tick) }?;

    Ok(Opened {
        client,
        tick: Some(tick),
        mode: AudioMode::ExcludingDiscord,
    })
}

/// Everything the machine is playing, Discord included: the declared fallback of
/// RF-30.
fn open_whole_system() -> Result<Opened, AudioError> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    // Loopback e capturar o que a placa esta tocando, entao o dispositivo e o de
    // saida, nao o de entrada.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }?;
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }?;
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            BUFFER_DURATION_HNS,
            0,
            &format(),
            None,
        )
    }?;
    // Sem evento aqui: num endpoint de saida em loopback o evento nao dispara
    // enquanto a maquina esta muda, e silencio e o estado normal de um jogo
    // entre um tiro e outro.
    Ok(Opened {
        client,
        tick: None,
        mode: AudioMode::WholeSystem,
    })
}

fn pump(
    opened: &Opened,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<AudioMode, AudioError>>,
    frames: &tokio::sync::mpsc::Sender<Vec<i16>>,
) -> Result<(), AudioError> {
    let client = &opened.client;
    let capture: IAudioCaptureClient = unsafe { client.GetService() }?;
    unsafe { client.Start() }?;
    let _ = ready.send(Ok(opened.mode));

    let mut dropped = 0u64;
    while !stop.load(Ordering::Relaxed) {
        match opened.tick {
            // O tempo limite nao e desperdicio: e o que faz o pedido de parada
            // ser atendido mesmo com a maquina muda.
            Some(tick) => {
                unsafe { WaitForSingleObject(tick, 100) };
            }
            None => std::thread::sleep(POLL),
        }

        loop {
            let available = unsafe { capture.GetNextPacketSize() }?;
            if available == 0 {
                break;
            }

            let mut data: *mut u8 = std::ptr::null_mut();
            let mut count = 0u32;
            let mut flags = 0u32;
            unsafe { capture.GetBuffer(&mut data, &mut count, &mut flags, None, None) }?;

            if count > 0 {
                let samples = count as usize * CHANNELS as usize;
                let mut buffer = vec![0i16; samples];
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                if !silent && !data.is_null() {
                    // PCM 16 bits intercalado, no formato pedido em `format()`.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            data.cast::<i16>(),
                            buffer.as_mut_ptr(),
                            samples,
                        );
                    }
                }
                // Silencio tambem vai: um fluxo com buracos faz o receptor
                // engasgar mais do que um fluxo de zeros.
                if frames.try_send(buffer).is_err() {
                    dropped += 1;
                    if dropped % 100 == 1 {
                        eprintln!("audio: encoder atrasado, {dropped} blocos descartados");
                    }
                }
            }

            unsafe { capture.ReleaseBuffer(count) }?;
        }
    }

    unsafe { client.Stop() }?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Process {
    pid: u32,
    parent: u32,
    name: String,
}

/// Discord ships under four names, and all four spawn the same process tree.
const DISCORD_IMAGES: [&str; 4] = [
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "discorddevelopment.exe",
];

fn discord_root_pid() -> Option<u32> {
    root_of_discord_tree(&snapshot())
}

/// The top of Discord's process tree.
///
/// Discord runs one main process with a handful of children — renderer, GPU,
/// audio service — all called `Discord.exe` too. Excluding a child would leave
/// the others audible, and the audio in particular comes from a child. So the
/// one to exclude is the one whose parent is not itself Discord.
fn root_of_discord_tree(processes: &[Process]) -> Option<u32> {
    let is_discord =
        |name: &str| DISCORD_IMAGES.contains(&name.to_ascii_lowercase().trim_end_matches(' '));

    let discord: Vec<&Process> = processes.iter().filter(|p| is_discord(&p.name)).collect();

    discord
        .iter()
        .find(|candidate| {
            // Pai fora da arvore do Discord (ou ja morto) significa raiz.
            !discord.iter().any(|other| other.pid == candidate.parent)
        })
        .or_else(|| discord.first())
        .map(|p| p.pid)
}

fn snapshot() -> Vec<Process> {
    let mut processes = Vec::new();
    let Ok(handle) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return processes;
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(handle, &mut entry) }.is_ok() {
        loop {
            let end = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            processes.push(Process {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..end]),
            });
            if unsafe { Process32NextW(handle, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(handle) };
    processes
}

#[cfg(test)]
mod tests {
    use super::*;
    use livekit::webrtc::audio_source::AudioSourceOptions;

    /// Touches real audio hardware, so it is not part of `just check`: a machine
    /// with no render endpoint has nothing to capture and would fail for the
    /// wrong reason.
    ///
    /// Run it by hand, from `desktop/src-tauri`, with Discord open and some
    /// sound playing:
    ///
    /// ```text
    /// cargo test -- --ignored --nocapture
    /// ```
    ///
    /// It is the only way to see this path work short of a full share, and it is
    /// what caught the `CoTaskMemAlloc` bug: COM activation, process loopback and
    /// the fallback are all invisible from the interface until someone complains
    /// they can hear themselves.
    #[tokio::test]
    #[ignore]
    async fn wasapi_delivers_samples() {
        let sink =
            NativeAudioSource::new(AudioSourceOptions::default(), SAMPLE_RATE, CHANNELS, 1_000);
        let (capture, mode) = start(sink).expect("a captura de audio deve iniciar");
        println!("modo de captura: {mode:?}");
        if mode == AudioMode::WholeSystem {
            println!("  (nenhum Discord rodando: nada a excluir, RF-30)");
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
        let delivered = capture.delivered_samples();
        capture.stop();

        println!(
            "amostras por canal em 3 s: {delivered} (esperado ~{})",
            SAMPLE_RATE * 3
        );
        assert!(
            delivered > u64::from(SAMPLE_RATE),
            "menos de um segundo de audio em tres: a captura abriu mas nao esta entregando"
        );

        // Se o heap tivesse sido corrompido pela ativacao, e aqui que a conta
        // chegaria. Foi exatamente assim que o defeito do blob apareceu.
        let churn: Vec<String> = (0..10_000).map(|i| format!("bloco {i}")).collect();
        assert_eq!(churn.len(), 10_000);
    }

    fn process(pid: u32, parent: u32, name: &str) -> Process {
        Process {
            pid,
            parent,
            name: name.to_owned(),
        }
    }

    #[test]
    fn with_no_discord_there_is_nothing_to_exclude() {
        // ADR-0025: sem Discord rodando a captura e do sistema inteiro, e isso
        // nao e falha.
        let processes = vec![process(4, 0, "System"), process(900, 4, "explorer.exe")];
        assert_eq!(root_of_discord_tree(&processes), None);
    }

    #[test]
    fn the_root_is_chosen_and_not_one_of_the_children() {
        // Excluir um filho deixaria os outros audiveis — e o audio vem
        // justamente de um processo filho.
        let processes = vec![
            process(900, 4, "explorer.exe"),
            process(1000, 900, "Discord.exe"),
            process(1001, 1000, "Discord.exe"),
            process(1002, 1000, "Discord.exe"),
        ];
        assert_eq!(root_of_discord_tree(&processes), Some(1000));
    }

    #[test]
    fn a_child_listed_before_its_parent_does_not_fool_the_search() {
        let processes = vec![
            process(1002, 1000, "Discord.exe"),
            process(1001, 1000, "Discord.exe"),
            process(1000, 900, "Discord.exe"),
            process(900, 4, "explorer.exe"),
        ];
        assert_eq!(root_of_discord_tree(&processes), Some(1000));
    }

    #[test]
    fn the_updater_is_not_mistaken_for_discord() {
        // Update.exe lanca o Discord e depois fica. Excluir a arvore dele
        // excluiria tambem o que mais ele tenha lancado.
        let processes = vec![
            process(800, 900, "Update.exe"),
            process(1000, 800, "Discord.exe"),
            process(1001, 1000, "Discord.exe"),
        ];
        assert_eq!(root_of_discord_tree(&processes), Some(1000));
    }

    #[test]
    fn the_ptb_and_canary_builds_count_as_discord() {
        assert_eq!(
            root_of_discord_tree(&[process(1000, 900, "DiscordPTB.exe")]),
            Some(1000)
        );
        assert_eq!(
            root_of_discord_tree(&[process(1000, 900, "DiscordCanary.exe")]),
            Some(1000)
        );
    }

    #[test]
    fn an_orphan_whose_parent_died_is_still_a_root() {
        // PID de pai que nao existe mais e comum; nao pode travar a busca.
        let processes = vec![process(1000, 31337, "Discord.exe")];
        assert_eq!(root_of_discord_tree(&processes), Some(1000));
    }

    #[test]
    fn a_program_merely_named_like_discord_is_left_alone() {
        let processes = vec![
            process(1000, 900, "DiscordOverlayHelper.exe"),
            process(1001, 900, "NotDiscord.exe"),
        ];
        assert_eq!(root_of_discord_tree(&processes), None);
    }
}
