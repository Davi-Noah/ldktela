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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use serde::Serialize;
use windows::core::{implement, Interface, Ref};
use windows::Win32::Media::Audio::{
    eConsole, eRender, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, BLOB, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Variant::VT_BLOB;

use crate::publisher::{CHANNELS, SAMPLE_RATE};

const BITS_PER_SAMPLE: u16 = 16;

/// 200 ms of slack, polled every 10 ms. Comfortably more than the ~16 ms the
/// Windows scheduler actually grants a sleeping thread.
const BUFFER_DURATION_HNS: i64 = 2_000_000;
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
}

impl AudioCapture {
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
            }
        }
    });

    let thread = std::thread::Builder::new()
        .name("ldkcord-audio".into())
        .spawn(move || run(&thread_stop, &ready_tx, &frames_tx))
        .map_err(|_| AudioError::Timeout)?;

    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(mode)) => Ok((
            AudioCapture {
                stop,
                thread: Some(thread),
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

    let outcome = open_client();
    let (client, mode) = match outcome {
        Ok(pair) => pair,
        Err(error) => {
            let _ = ready.send(Err(error));
            unsafe { CoUninitialize() };
            return;
        }
    };

    if let Err(error) = pump(&client, stop, ready, frames, mode) {
        eprintln!("audio: captura interrompida: {error}");
    }
    unsafe { CoUninitialize() };
}

/// Process loopback if Discord is running, whole-system loopback otherwise.
fn open_client() -> Result<(IAudioClient, AudioMode), AudioError> {
    match discord_root_pid() {
        Some(pid) => {
            match activate_process_loopback(pid) {
                Ok(client) => Ok((client, AudioMode::ExcludingDiscord)),
                Err(error) => {
                    // RF-30: cair para o sistema inteiro e melhor do que nao ter
                    // audio, desde que a interface diga o que esta acontecendo.
                    eprintln!("audio: process loopback indisponivel ({error}), caindo para o sistema inteiro");
                    Ok((activate_whole_system()?, AudioMode::WholeSystem))
                }
            }
        }
        None => Ok((activate_whole_system()?, AudioMode::WholeSystem)),
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
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion(Arc<(Mutex<bool>, Condvar)>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let (lock, signal) = &*self.0;
        if let Ok(mut done) = lock.lock() {
            *done = true;
        }
        signal.notify_all();
        Ok(())
    }
}

fn activate_process_loopback(pid: u32) -> Result<IAudioClient, AudioError> {
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };

    let mut activation = PROPVARIANT::default();
    // O PROPVARIANT carrega a struct como blob cru; nao ha construtor tipado
    // para isto no windows-rs, e a API do Windows nao aceita outra forma.
    unsafe {
        let inner = &mut activation.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob = BLOB {
            cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
            pBlobData: std::ptr::from_mut(&mut params).cast::<u8>(),
        };
    }

    let state = Arc::new((Mutex::new(false), Condvar::new()));
    let handler: IActivateAudioInterfaceCompletionHandler = Completion(Arc::clone(&state)).into();

    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&activation),
            &handler,
        )
    }?;

    {
        let (lock, signal) = &*state;
        let guard = lock.lock().map_err(|_| AudioError::Timeout)?;
        let (_guard, timeout) = signal
            .wait_timeout_while(guard, Duration::from_secs(3), |done| !*done)
            .map_err(|_| AudioError::Timeout)?;
        if timeout.timed_out() {
            return Err(AudioError::Timeout);
        }
    }

    let mut result = windows::core::HRESULT(0);
    let mut interface: Option<windows::core::IUnknown> = None;
    unsafe { operation.GetActivateResult(&mut result, &mut interface) }?;
    result.ok()?;
    let client: IAudioClient = interface
        .ok_or_else(|| AudioError::Windows("o Windows nao devolveu um IAudioClient".into()))?
        .cast()?;

    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            BUFFER_DURATION_HNS,
            0,
            &format(),
            None,
        )
    }?;
    Ok(client)
}

fn activate_whole_system() -> Result<IAudioClient, AudioError> {
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
    Ok(client)
}

/// Polls instead of waiting on an event.
///
/// Event-driven loopback is the documented shape for a capture endpoint, but on
/// a render endpoint in loopback the event does not fire while the machine is
/// silent — and silence is the normal state of a game between gunshots. Polling
/// is one code path that behaves the same in both modes.
fn pump(
    client: &IAudioClient,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<AudioMode, AudioError>>,
    frames: &tokio::sync::mpsc::Sender<Vec<i16>>,
    mode: AudioMode,
) -> Result<(), AudioError> {
    let capture: IAudioCaptureClient = unsafe { client.GetService() }?;
    unsafe { client.Start() }?;
    let _ = ready.send(Ok(mode));

    let mut dropped = 0u64;
    while !stop.load(Ordering::Relaxed) {
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
                    // WASAPI entregou PCM 16 bits intercalado, no formato que
                    // pedimos em `format()`.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            data.cast::<i16>(),
                            buffer.as_mut_ptr(),
                            samples,
                        );
                    }
                }
                // Silencio tambem e enviado: um fluxo com buracos faz o receptor
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
        std::thread::sleep(POLL);
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
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
    processes
}

#[cfg(test)]
mod tests {
    use super::*;

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
