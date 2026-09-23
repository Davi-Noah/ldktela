//! The LiveKit connection that publishes the screen (ADR-0026, ADR-0027).
//!
//! This is a second, separate connection to the room: the WebView is already in
//! there watching, under the canonical identity, and LiveKit disconnects the
//! first participant when a second one arrives with the same identity. The
//! server hands out the suffixed identity inside the publish token; nothing here
//! chooses it.
//!
//! The connection exists only while sharing. An idle app holds no media
//! connection at all (RNF-03).

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use livekit::options::{TrackPublishOptions, VideoCodec, VideoEncoding};
use livekit::track::{LocalAudioTrack, LocalTrack, LocalVideoTrack, TrackSource};
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::AudioSourceOptions;
use livekit::webrtc::prelude::{RtcAudioSource, RtcVideoSource};
use livekit::webrtc::rtp_parameters::DegradationPreference;
use livekit::webrtc::stats::{
    IceCandidateType, OutboundRtpStats, QualityLimitationReason, RtcStats,
};
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::VideoResolution;
use livekit::id::TrackSid;
use livekit::{Room, RoomEvent, RoomOptions};
use serde::{Deserialize, Serialize};

use crate::capture::Size;

/// What the publisher encodes (RF-36).
///
/// Resolution and frame rate belong to whoever pays for the encode; the viewer
/// picks among the layers that come out (ADR-0023). These numbers moved here
/// from `desktop/src/media/tracks.ts` when publishing moved to the core: they
/// belong next to the encoder, not next to the button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    #[serde(rename = "1080p60")]
    P1080p60,
    #[serde(rename = "1080p30")]
    P1080p30,
    #[serde(rename = "720p60")]
    P720p60,
    #[serde(rename = "720p30")]
    P720p30,
}

impl Preset {
    /// ~6 Mbps at 1080p60 is the figure the egress budget in RNF-05 is written
    /// against.
    pub fn ceiling(self) -> Size {
        match self {
            Preset::P1080p60 | Preset::P1080p30 => Size {
                width: 1920,
                height: 1080,
            },
            Preset::P720p60 | Preset::P720p30 => Size {
                width: 1280,
                height: 720,
            },
        }
    }

    pub fn fps(self) -> u32 {
        match self {
            Preset::P1080p60 | Preset::P720p60 => 60,
            Preset::P1080p30 | Preset::P720p30 => 30,
        }
    }

    pub fn max_bitrate(self) -> u64 {
        match self {
            Preset::P1080p60 => 6_000_000,
            Preset::P1080p30 => 4_000_000,
            Preset::P720p60 => 3_000_000,
            Preset::P720p30 => 1_800_000,
        }
    }
}

/// One spatial layer, three temporal ones (ADR-0032).
///
/// Not a tuning knob: it is the only shape of VP9 ladder this stack actually
/// delivers. See the comment at the publish call, and the sweep that measured
/// the alternatives.
const SCALABILITY_MODE: &str = "L1T3";

/// Opus runs at 48 kHz; asking the capture for anything else only inserts a
/// resampler between the sound card and the encoder.
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("o servidor de midia recusou a conexao: {0}")]
    Connect(String),
    #[error("nao consegui publicar a tela: {0}")]
    Publish(String),
}

/// Sampled on an interval by the interface, never in the media path (RF-21,
/// RF-22, `CLAUDE.md` §7).
#[derive(Debug, Clone, Default, Serialize)]
pub struct PublisherStats {
    pub bitrate_kbps: u32,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    /// True when libwebrtc chose a hardware encoder. Worth surfacing: it is the
    /// difference between a core of CPU and almost none, and until the move to
    /// the core it was not even observable.
    pub hardware_encoder: bool,
    /// `none`, `cpu`, `bandwidth` or `other` — the encoder itself saying why it
    /// is holding back. It is the difference between "the network cannot" and
    /// "this machine cannot", which produce the same low frame rate on screen.
    pub limited_by: &'static str,
    /// `udp`, `tcp`, `relay/udp`… — where the media is actually going. Falling
    /// back to TCP wrecks quality on its own, and it is a symptom of a blocked
    /// UDP port rather than of a bad network.
    pub transport: String,
    pub rtt_ms: u32,
    /// What the congestion controller believes the uplink has.
    pub available_kbps: u32,
    /// Frames converted from the screen and offered to the encoder.
    ///
    /// Together with a zero bitrate it separates the two failures that look the
    /// same: a dead capture produces nothing, while a stream nobody is watching
    /// produces plenty and has every frame refused.
    pub captured_frames: u64,
    /// Of those, the ones the encoder accepted.
    ///
    /// Equal to `captured_frames` when the stream is live; stuck at zero while
    /// it is paused for want of a subscriber. The gap is the diagnosis.
    pub encoded_frames: u64,
    /// Samples per channel captured so far, when sharing with audio.
    ///
    /// It is the only thing that tells a muted game apart from a broken capture:
    /// both sound like silence to everyone watching, and only one is our fault.
    pub audio_samples: Option<u64>,
}

/// Which path the media is actually taking, read from the nominated ICE pair.
///
/// Exists because "the quality dropped" has three unrelated causes that look
/// identical from the outside — a congested uplink, a starved encoder, and media
/// that fell back to TCP because UDP is blocked — and only the third one is
/// fixed by opening a port.
#[derive(Debug, Default)]
struct NetworkPath {
    transport: String,
    rtt_ms: u32,
    available_kbps: u32,
}

impl NetworkPath {
    fn from(report: &[RtcStats]) -> Self {
        // O transporte aponta o par escolhido pelo nome; `nominated` e o plano B,
        // porque mais de um par pode estar nomeado durante uma renegociacao e
        // ler o errado descreveria um caminho que ninguem esta usando.
        let selected = report.iter().find_map(|entry| match entry {
            RtcStats::Transport(transport)
                if !transport.transport.selected_candidate_pair_id.is_empty() =>
            {
                Some(transport.transport.selected_candidate_pair_id.clone())
            }
            _ => None,
        });

        let Some(pair) = report.iter().find_map(|entry| match entry {
            RtcStats::CandidatePair(pair) => match &selected {
                Some(id) => (pair.rtc.id == *id).then_some(pair),
                None => pair.candidate_pair.nominated.then_some(pair),
            },
            _ => None,
        }) else {
            return Self {
                transport: "—".to_owned(),
                ..Default::default()
            };
        };

        let local = report.iter().find_map(|entry| match entry {
            RtcStats::LocalCandidate(candidate)
                if candidate.rtc.id == pair.candidate_pair.local_candidate_id =>
            {
                Some(&candidate.local_candidate)
            }
            _ => None,
        });

        let transport = match local {
            Some(candidate) => {
                let protocol = candidate.protocol.to_ascii_lowercase();
                match candidate.candidate_type {
                    // Relay significa TURN: a midia passa por um intermediario,
                    // o que custa latencia e banda mas atravessa CGNAT.
                    Some(IceCandidateType::Relay) => format!("relay/{protocol}"),
                    _ => protocol,
                }
            }
            None => "?".to_owned(),
        };

        Self {
            transport,
            // O RTT vem em segundos.
            rtt_ms: (pair.candidate_pair.current_round_trip_time * 1000.0).round() as u32,
            available_kbps: (pair.candidate_pair.available_outgoing_bitrate / 1000.0).round()
                as u32,
        }
    }
}

/// What the camera publishes, fixed (ADR-0038).
///
/// Sem seletor de preset: é um rosto, não texto de 9 px em movimento. 1080p numa
/// webcam gasta egress para transmitir o grão do sensor, e 60 fps gasta o dobro
/// para mostrar alguém sentado.
pub const CAMERA_SIZE: Size = Size {
    width: 1280,
    height: 720,
};
pub const CAMERA_FPS: u32 = 30;
/// Um rosto a 720p30 cabe folgado aqui; o mesmo número numa tela de jogo não
/// caberia, e é por isso que a câmera tem o seu.
const CAMERA_MAX_BITRATE: u64 = 1_600_000;

/// Which publication a call is about (ADR-0038).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Screen,
    Camera,
}

impl Source {
    fn stream(self) -> &'static str {
        match self {
            Source::Screen => "screen",
            Source::Camera => "camera",
        }
    }
}

/// The sids of what one source has published, so it can be taken down alone.
#[derive(Default)]
struct Published {
    video: Option<TrackSid>,
    audio: Option<TrackSid>,
}

pub struct Publisher {
    room: Arc<Room>,
    screen: StdMutex<Published>,
    camera: StdMutex<Published>,
    /// Last (bytes_sent, instant) seen, so bitrate is a delta and not a total.
    last_sample: StdMutex<Option<(u64, Instant)>>,
}

impl Publisher {
    /// Connects without publishing anything.
    ///
    /// Uma conexão, duas trilhas possíveis (ADR-0027 e ADR-0038): a câmera entra
    /// e sai por cima da mesma sessão, e é por isso que conectar deixou de vir
    /// junto com publicar.
    pub async fn connect(
        url: &str,
        token: &str,
        on_disconnect: impl Fn(String) + Send + 'static,
    ) -> Result<Self, PublishError> {
        // `RoomOptions` e `non_exhaustive`: o SDK reserva o direito de acrescentar
        // campos, entao nao ha literal de struct possivel aqui.
        #[allow(clippy::field_reassign_with_default)]
        let options = {
            let mut options = RoomOptions::default();
            // Esta conexao so publica. Assinar aqui baixaria as telas dos outros
            // uma segunda vez, ja que o WebView tambem esta na sala.
            options.auto_subscribe = false;
            options.adaptive_stream = false;
            // Deixa o servidor mandar pausar camada que ninguem esta vendo. E a
            // metade do publicador da economia de egress do RF-32.
            options.dynacast = true;
            options
        };

        let (room, mut events) = Room::connect(url, token, options)
            .await
            .map_err(|error| PublishError::Connect(error.to_string()))?;
        let room = Arc::new(room);

        // O canal e ilimitado: sem alguem drenando, ele cresce enquanto a sessao
        // durar. E e por aqui que se descobre que o SFU nos derrubou.
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    RoomEvent::Disconnected { reason } => {
                        on_disconnect(format!("{reason:?}"));
                        return;
                    }
                    RoomEvent::Reconnecting => eprintln!("publicacao: reconectando"),
                    RoomEvent::Reconnected => eprintln!("publicacao: reconectado"),
                    _ => {}
                }
            }
        });

        Ok(Self {
            room,
            screen: StdMutex::new(Published::default()),
            camera: StdMutex::new(Published::default()),
            last_sample: StdMutex::new(None),
        })
    }

    /// Publishes the screen, and its audio when asked.
    ///
    /// The track is published before any frame arrives on purpose: it is what
    /// produces the `track_published` webhook, and therefore `SHARE_START` and
    /// the "on air" clock every viewer reads (RF-34).
    pub async fn publish_screen(
        &self,
        preset: Preset,
        with_audio: bool,
    ) -> Result<(NativeVideoSource, Option<NativeAudioSource>), PublishError> {
        let ceiling = preset.ceiling();
        let video = NativeVideoSource::new(
            VideoResolution {
                width: ceiling.width,
                height: ceiling.height,
            },
            // is_screencast: muda a sintonia do encoder e desliga heuristicas de
            // camera. Sem isso, texto parado fica borrado.
            true,
        );
        let track =
            LocalVideoTrack::create_video_track("screen", RtcVideoSource::Native(video.clone()));

        let publication = self
            .room
            .local_participant()
            .publish_track(
                LocalTrack::Video(track),
                TrackPublishOptions {
                    source: TrackSource::Screenshare,
                    video_codec: VideoCodec::VP9,
                    // Uma unica camada espacial, tres temporais (ADR-0032).
                    //
                    // As outras duas formas de pedir uma escada ao VP9 estao
                    // quebradas nesta pilha, e as duas falham em silencio:
                    // simulcast (mais de uma codificacao RTP) entrega **11 fps**
                    // de uma captura de 60, e SVC espacial (`L2T3_KEY`,
                    // `L3T3_KEY`) entrega **zero quadro** ao espectador. Medido
                    // por `sweep_publish_options_against_a_real_subscriber`, que
                    // conta no espectador porque nenhuma estatistica do
                    // publicador distingue os tres casos — `limited_by` diz
                    // `none` nos tres.
                    //
                    // `L1T3` chega aos 60 fps e ainda deixa o SFU cortar a
                    // metade ou um quarto do relogio para quem nao aguenta.
                    simulcast: false,
                    scalability_mode: Some(SCALABILITY_MODE.to_owned()),
                    video_encoding: Some(VideoEncoding {
                        max_bitrate: preset.max_bitrate(),
                        max_framerate: f64::from(preset.fps()),
                    }),
                    // O SDK usa MaintainResolution para tela, presumindo planilha.
                    // Aqui e jogo: movimento importa mais que nitidez, e a escolha
                    // veio junto do codigo que saiu do WebView.
                    degradation_preference: Some(DegradationPreference::MaintainFramerate),
                    stream: Source::Screen.stream().to_owned(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| PublishError::Publish(error.to_string()))?;
        if let Ok(mut held) = self.screen.lock() {
            held.video = Some(publication.sid());
        }

        let audio = if with_audio {
            let source = NativeAudioSource::new(
                AudioSourceOptions {
                    // Nada disto se aplica: a fonte e o mixer do sistema, nao um
                    // microfone. Ligados, comeriam o grave do jogo.
                    echo_cancellation: false,
                    noise_suppression: false,
                    auto_gain_control: false,
                },
                SAMPLE_RATE,
                CHANNELS,
                // Fila de 1 s: a captura do WASAPI e em rajadas de ~10 ms e o
                // encoder consome em ritmo proprio.
                1_000,
            );
            let track = LocalAudioTrack::create_audio_track(
                "screen-audio",
                RtcAudioSource::Native(source.clone()),
            );
            let publication = self
                .room
                .local_participant()
                .publish_track(
                    LocalTrack::Audio(track),
                    TrackPublishOptions {
                        source: TrackSource::ScreenshareAudio,
                        // Audio de jogo nao tem silencio para o DTX cortar, e o
                        // corte engole o ataque das notas.
                        dtx: false,
                        red: false,
                        stream: Source::Screen.stream().to_owned(),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|error| PublishError::Publish(error.to_string()))?;
            if let Ok(mut held) = self.screen.lock() {
                held.audio = Some(publication.sid());
            }
            Some(source)
        } else {
            None
        };

        Ok((video, audio))
    }

    /// Publishes the camera (ADR-0038). Nunca carrega audio: a voz e do Discord.
    pub async fn publish_camera(&self) -> Result<NativeVideoSource, PublishError> {
        let video = NativeVideoSource::new(
            VideoResolution {
                width: CAMERA_SIZE.width,
                height: CAMERA_SIZE.height,
            },
            // `false`, ao contrario da tela: aqui as heuristicas de camera do
            // encoder sao as certas — movimento continuo, ruido de sensor, e
            // nada de texto parado para preservar.
            false,
        );
        let track =
            LocalVideoTrack::create_video_track("camera", RtcVideoSource::Native(video.clone()));

        let publication = self
            .room
            .local_participant()
            .publish_track(
                LocalTrack::Video(track),
                TrackPublishOptions {
                    source: TrackSource::Camera,
                    video_codec: VideoCodec::VP9,
                    // A mesma escada temporal da tela, e pelo mesmo motivo
                    // (ADR-0032): e a unica que esta pilha entrega de verdade.
                    simulcast: false,
                    scalability_mode: Some(SCALABILITY_MODE.to_owned()),
                    video_encoding: Some(VideoEncoding {
                        max_bitrate: CAMERA_MAX_BITRATE,
                        max_framerate: f64::from(CAMERA_FPS),
                    }),
                    // Rosto: perder nitidez incomoda menos do que perder
                    // fluidez, mesma escolha da tela.
                    degradation_preference: Some(DegradationPreference::MaintainFramerate),
                    stream: Source::Camera.stream().to_owned(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| PublishError::Publish(error.to_string()))?;
        if let Ok(mut held) = self.camera.lock() {
            held.video = Some(publication.sid());
        }
        Ok(video)
    }

    /// Takes one source off the air, leaving the other one running.
    pub async fn unpublish(&self, source: Source) {
        let slot = match source {
            Source::Screen => &self.screen,
            Source::Camera => &self.camera,
        };
        let sids = match slot.lock() {
            Ok(mut held) => (held.video.take(), held.audio.take()),
            Err(_) => (None, None),
        };
        for sid in [sids.0, sids.1].into_iter().flatten() {
            if let Err(error) = self.room.local_participant().unpublish_track(&sid).await {
                eprintln!("publicacao: erro ao despublicar {sid}: {error}");
            }
        }
    }


    /// Bitrate, frame rate and the resolution actually being encoded.
    ///
    /// Summed across every spatial layer: with SVC the encoder produces several,
    /// and reporting only one would understate what the upload is costing. The
    /// resolution reported is the widest layer, which is what the top viewer
    /// sees.
    pub async fn stats(&self) -> PublisherStats {
        // As estatisticas da conexao inteira, e nao so as da track: e no par de
        // candidatos que mora a resposta para "por que a qualidade caiu" — se a
        // midia esta em UDP ou caiu para TCP, qual o RTT, e quanto de banda o
        // controle de congestionamento acha que tem. Sem isso, banda ruim e CPU
        // insuficiente produzem exatamente o mesmo numero na tela.
        let Ok(report) = self.room.get_stats().await else {
            return PublisherStats::default();
        };
        let report = report.publisher_stats;

        let mut bytes_sent = 0u64;
        let mut widest: Option<&OutboundRtpStats> = None;
        for entry in &report {
            let RtcStats::OutboundRtp(outbound) = entry else {
                continue;
            };
            bytes_sent += outbound.sent.bytes_sent;
            if widest.is_none_or(|w| outbound.outbound.frame_width > w.outbound.frame_width) {
                widest = Some(outbound);
            }
        }

        let path = NetworkPath::from(&report);

        let now = Instant::now();
        let bitrate_kbps = match self.last_sample.lock() {
            Ok(mut last) => {
                let previous = last.replace((bytes_sent, now));
                previous
                    .and_then(|(bytes, at)| {
                        let elapsed = now.saturating_duration_since(at).as_secs_f64();
                        (elapsed > 0.0).then(|| {
                            (bytes_sent.saturating_sub(bytes) as f64 * 8.0 / elapsed / 1000.0)
                                .round() as u32
                        })
                    })
                    .unwrap_or(0)
            }
            Err(_) => 0,
        };

        let Some(widest) = widest else {
            return PublisherStats {
                bitrate_kbps,
                transport: path.transport,
                rtt_ms: path.rtt_ms,
                available_kbps: path.available_kbps,
                ..Default::default()
            };
        };
        PublisherStats {
            bitrate_kbps,
            fps: widest.outbound.frames_per_second.round() as u32,
            width: widest.outbound.frame_width,
            height: widest.outbound.frame_height,
            hardware_encoder: widest.outbound.power_efficient_encoder,
            limited_by: match widest.outbound.quality_limitation_reason {
                QualityLimitationReason::None => "none",
                QualityLimitationReason::Cpu => "cpu",
                QualityLimitationReason::Bandwidth => "bandwidth",
                QualityLimitationReason::Other => "other",
            },
            transport: path.transport,
            rtt_ms: path.rtt_ms,
            available_kbps: path.available_kbps,
            // Preenchidos por quem tem a captura em maos.
            captured_frames: 0,
            encoded_frames: 0,
            audio_samples: None,
        }
    }

    /// Leaves the room, which unpublishes everything.
    ///
    /// Closing beats unpublishing track by track: the connection has no other
    /// purpose, and leaving is also what the server sees if the process dies, so
    /// there is one path to test instead of two.
    pub async fn stop(self) {
        if let Err(error) = self.room.close().await {
            eprintln!("publicacao: erro ao sair da sala: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{self, SourceKind};
    use std::time::Duration;

    /// O SFU de desenvolvimento, e o padrao de todos os testes daqui.
    ///
    /// `LK_URL`, `LK_KEY` e `LK_SECRET` apontam a medicao para outro servidor —
    /// e o que permite rodar `measure_1080p60_end_to_end` contra a VM de
    /// producao, que e a unica forma de saber se a implantacao aguenta o que o
    /// aplicativo promete. Em loopback, o caminho de rede e um caminho que
    /// nenhum usuario percorre.
    ///
    /// ```text
    /// LK_URL=ws://IP:7880 LK_KEY=<chave> LK_SECRET=<segredo> \
    ///   cargo test --release -- --ignored --nocapture measure_1080p60
    /// ```
    fn sfu_url() -> String {
        std::env::var("LK_URL").unwrap_or_else(|_| "ws://127.0.0.1:7880".to_owned())
    }

    fn dev_token(room: &str, identity: &str, publish: bool) -> String {
        use livekit_api::access_token::{AccessToken, VideoGrants};
        let key = std::env::var("LK_KEY").unwrap_or_else(|_| "devkey".to_owned());
        let secret = std::env::var("LK_SECRET")
            .unwrap_or_else(|_| "dev-only-not-a-real-key-0123456789abcdef".to_owned());
        AccessToken::with_api_key(&key, &secret)
            .with_identity(identity)
            .with_name(identity)
            .with_grants(VideoGrants {
                room_join: true,
                room: room.to_owned(),
                can_subscribe: true,
                can_publish: publish,
                ..Default::default()
            })
            .to_jwt()
            .expect("token de desenvolvimento")
    }

    /// Publishes a real screen to the development SFU and checks that bytes
    /// actually leave (RF-13, RNF-05).
    ///
    /// Needs `just infra-up`, so it is not part of `just check`. Run it from
    /// `desktop/src-tauri`:
    ///
    /// ```text
    /// cargo test -- --ignored --nocapture
    /// ```
    ///
    /// It exists because every cheaper check passes while the product shows a
    /// black screen: the capture produces frames, the track is published, the
    /// webhook fires and the viewer subscribes — and the publisher panel still
    /// reads 0 kb/s. The one thing none of them prove is that a frame reached
    /// the encoder, and a subscriber has to be present for that to be true at
    /// all, because dynacast pauses an unwatched track.
    #[tokio::test]
    #[ignore]
    async fn a_real_screen_reaches_the_sfu() {
        let room_name = format!("dvc-test-{}", std::process::id());

        // O espectador entra primeiro: sem ninguem assinando, o dynacast mantem
        // o encoder pausado e a medicao seria de um caminho que o produto nunca
        // usa.
        let (_viewer, mut viewer_events) = Room::connect(
            &sfu_url(),
            &dev_token(&room_name, "espectador", false),
            RoomOptions::default(),
        )
        .await
        .expect("o espectador deve conectar; rode `just infra-up`");

        // Contar no espectador e a unica prova de que a midia atravessou: tudo
        // do lado do publicador pode parecer certo sem um byte sair.
        let received = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counted = Arc::clone(&received);
        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            use livekit::webrtc::video_stream::native::NativeVideoStream;
            while let Some(event) = viewer_events.recv().await {
                if let livekit::RoomEvent::TrackSubscribed { track, .. } = event {
                    eprintln!("espectador: assinou {:?}", track.kind());
                    if let livekit::track::RemoteTrack::Video(video) = track {
                        let counted = Arc::clone(&counted);
                        tokio::spawn(async move {
                            let mut stream = NativeVideoStream::new(video.rtc_track());
                            while let Some(frame) = stream.next().await {
                                let _ = frame;
                                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        });
                    }
                }
            }
        });

        let preset = Preset::P720p30;
        let publisher = Publisher::connect(
            &sfu_url(),
            &dev_token(&room_name, "tester~pub", true),
            |reason| eprintln!("publicacao encerrada: {reason}"),
        )
        .await
        .expect("o publicador deve conectar");
        let (video_sink, _) = publisher
            .publish_screen(preset, false)
            .await
            .expect("a tela deve publicar");

        let screen = capture::list_sources(&[])
            .into_iter()
            .find(|s| s.kind == SourceKind::Screen)
            .expect("deve existir uma tela");
        let capture = capture::start(
            SourceKind::Screen,
            screen.id.parse().expect("id numerico"),
            preset.ceiling(),
            preset.fps(),
            video_sink.clone(),
            None,
            || eprintln!("fonte perdida"),
        )
        .expect("a captura deve iniciar");

        // O primeiro `stats()` so estabelece a linha de base do bitrate, que e
        // uma diferenca entre duas leituras.
        tokio::time::sleep(Duration::from_secs(4)).await;
        let _ = publisher.stats().await;
        tokio::time::sleep(Duration::from_secs(4)).await;

        let frames = capture.delivered_frames();
        let stats = publisher.stats().await;
        capture.stop();
        publisher.stop().await;

        let got = received.load(std::sync::atomic::Ordering::Relaxed);
        println!("quadros aceitos pelo encoder: {frames}");
        println!("quadros recebidos pelo espectador: {got}");
        println!("{stats:?}");

        assert!(
            frames > 0,
            "o encoder recusou todos os quadros: nada foi codificado"
        );
        assert!(got > 0, "o espectador nao recebeu quadro nenhum");
        assert!(
            stats.bitrate_kbps > 0 && stats.fps > 0,
            "o espectador recebeu {got} quadros, mas o painel do publicador              mostraria zero: {stats:?}"
        );
    }

    /// What a subscriber actually receives, at a given preset (RNF-05).
    ///
    /// This is the only measurement that answers "is 1080p60 real". Everything
    /// the publisher can see about itself is consistent with a viewer watching a
    /// 3 fps slideshow: the encoder reports the widest layer it produces, while
    /// the SFU may be forwarding a different one entirely.
    ///
    /// Needs `just infra-up`. Run from `desktop/src-tauri`:
    ///
    /// ```text
    /// cargo test --release -- --ignored --nocapture measure
    /// ```
    ///
    /// `--release` is not optional for this one: at 1080p60 the capture thread
    /// has 16 ms per frame to convert and scale, and a debug build spends more
    /// than that, so a debug run measures the build and not the product.
    struct Received {
        frames: u64,
        width: u32,
        height: u32,
    }

    async fn measure(preset: Preset, seconds: u64) -> (Received, PublisherStats, u64, u64) {
        use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

        let room_name = format!("dvc-measure-{}-{:?}", std::process::id(), preset);

        // `adaptive_stream` fica desligado neste espectador (e o padrao do SDK
        // Rust), entao ele pede a camada de cima. E de proposito: aqui se mede o
        // teto do caminho, e a escolha de camada do espectador de verdade tem o
        // seu proprio teste, do lado do WebView.
        let (_viewer, mut viewer_events) = Room::connect(
            &sfu_url(),
            &dev_token(&room_name, "espectador", false),
            RoomOptions::default(),
        )
        .await
        .expect("o espectador deve conectar; rode `just infra-up`");

        let frames = Arc::new(AtomicU64::new(0));
        let width = Arc::new(AtomicU32::new(0));
        let height = Arc::new(AtomicU32::new(0));
        let (counted, wide, tall) = (Arc::clone(&frames), Arc::clone(&width), Arc::clone(&height));
        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            use livekit::webrtc::video_stream::native::NativeVideoStream;
            while let Some(event) = viewer_events.recv().await {
                if let livekit::RoomEvent::TrackSubscribed {
                    track: livekit::track::RemoteTrack::Video(video),
                    ..
                } = event
                {
                    let (counted, wide, tall) =
                        (Arc::clone(&counted), Arc::clone(&wide), Arc::clone(&tall));
                    tokio::spawn(async move {
                        let mut stream = NativeVideoStream::new(video.rtc_track());
                        while let Some(frame) = stream.next().await {
                            let buffer = frame.buffer;
                            wide.store(buffer.width(), Ordering::Relaxed);
                            tall.store(buffer.height(), Ordering::Relaxed);
                            counted.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
            }
        });

        let publisher = Publisher::connect(
            &sfu_url(),
            &dev_token(&room_name, "medidor~pub", true),
            |reason| eprintln!("publicacao encerrada: {reason}"),
        )
        .await
        .expect("o publicador deve conectar");
        let (video_sink, _) = publisher
            .publish_screen(preset, false)
            .await
            .expect("a tela deve publicar");

        let screen = capture::list_sources(&[])
            .into_iter()
            .find(|s| s.kind == SourceKind::Screen)
            .expect("deve existir uma tela");
        let capture = capture::start(
            SourceKind::Screen,
            screen.id.parse().expect("id numerico"),
            preset.ceiling(),
            preset.fps(),
            video_sink.clone(),
            None,
            || eprintln!("fonte perdida"),
        )
        .expect("a captura deve iniciar");

        // Dois segundos de aquecimento que nao entram na conta: o encoder sobe
        // de bitrate devagar de proposito, e medir a rampa acusaria o produto de
        // um defeito que ele nao tem.
        tokio::time::sleep(Duration::from_secs(2)).await;
        let _ = publisher.stats().await;
        frames.store(0, Ordering::Relaxed);
        let base_produced = capture.produced_frames();
        let base_delivered = capture.delivered_frames();

        let started = Instant::now();
        // Amostra a cada 2 s em vez de esperar o fim: uma media de dez segundos
        // nao distingue "o encoder esta travado em 300 kb/s" de "o controle de
        // congestionamento esta subindo devagar" — e sao problemas diferentes,
        // com correcoes diferentes.
        let mut last_frames = 0u64;
        // A ultima amostra e a que vale: `bitrate_kbps` e uma diferenca entre
        // duas leituras, entao chamar `stats()` de novo aqui mediria uma janela
        // de milissegundos e reportaria zero.
        let mut stats = PublisherStats::default();
        for _ in 0..(seconds / 2) {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let now = frames.load(Ordering::Relaxed);
            let sample = publisher.stats().await;
            println!(
                "  t+{:>4.1}s  espectador {:>4.1} /s  |  encoder {:>5} kb/s {:>2} fps {}x{}  |  banda {:>5} kb/s  |  limite {}",
                started.elapsed().as_secs_f64(),
                (now - last_frames) as f64 / 2.0,
                sample.bitrate_kbps,
                sample.fps,
                sample.width,
                sample.height,
                sample.available_kbps,
                sample.limited_by,
            );
            last_frames = now;
            stats = sample;
        }
        let elapsed = started.elapsed().as_secs_f64();

        let produced = capture.produced_frames() - base_produced;
        let delivered = capture.delivered_frames() - base_delivered;
        let got = Received {
            frames: frames.load(Ordering::Relaxed),
            width: width.load(Ordering::Relaxed),
            height: height.load(Ordering::Relaxed),
        };
        capture.stop();
        publisher.stop().await;

        println!("--------------------------------------------------");
        println!("preset            {preset:?} (teto {:?})", preset.ceiling());
        println!("janela            {elapsed:.1} s");
        println!(
            "captura           {produced} quadros produzidos ({:.1} /s), {delivered} aceitos pelo encoder",
            produced as f64 / elapsed
        );
        println!(
            "espectador        {} quadros ({:.1} /s) a {}x{}",
            got.frames,
            got.frames as f64 / elapsed,
            got.width,
            got.height
        );
        println!(
            "encoder           {} kb/s, {} fps, {}x{}, {}, limitado por {}",
            stats.bitrate_kbps,
            stats.fps,
            stats.width,
            stats.height,
            if stats.hardware_encoder {
                "hardware"
            } else {
                "software"
            },
            stats.limited_by
        );
        println!(
            "caminho           {} - {} ms - {} kb/s disponiveis",
            stats.transport, stats.rtt_ms, stats.available_kbps
        );
        println!("--------------------------------------------------");

        (got, stats, produced, delivered)
    }

    /// An experiment, not a test of the product: it publishes the same real
    /// screen several times with different encoder options and reports what a
    /// subscriber receives from each.
    ///
    /// It exists because `measure_1080p60_end_to_end` says the frame rate is
    /// wrong without saying which knob is wrong, and the candidates — the codec,
    /// the simulcast ladder, the degradation preference — are indistinguishable
    /// from the publisher's own statistics.
    ///
    /// ```text
    /// cargo test --release -- --ignored --nocapture sweep
    /// ```
    #[tokio::test]
    #[ignore]
    async fn sweep_publish_options_against_a_real_subscriber() {
        use livekit::options::VideoPreset;

        let base = |codec: VideoCodec| TrackPublishOptions {
            source: TrackSource::Screenshare,
            video_codec: codec,
            simulcast: true,
            scalability_mode: None,
            video_encoding: Some(VideoEncoding {
                max_bitrate: 6_000_000,
                max_framerate: 60.0,
            }),
            degradation_preference: Some(DegradationPreference::MaintainFramerate),
            stream: "screen".to_owned(),
            ..Default::default()
        };

        let variants: Vec<(&str, TrackPublishOptions)> = vec![
            (
                "vp9 + escada padrao (produto de hoje)",
                base(VideoCodec::VP9),
            ),
            (
                "vp9 sem simulcast",
                TrackPublishOptions {
                    simulcast: false,
                    ..base(VideoCodec::VP9)
                },
            ),
            (
                "vp9 + escada de 720p30",
                TrackPublishOptions {
                    simulcast_layers: Some(vec![VideoPreset::new(1280, 720, 2_000_000, 30.0)]),
                    ..base(VideoCodec::VP9)
                },
            ),
            ("vp8 + escada padrao", base(VideoCodec::VP8)),
            (
                "vp8 sem simulcast",
                TrackPublishOptions {
                    simulcast: false,
                    ..base(VideoCodec::VP8)
                },
            ),
            (
                "h264 sem simulcast",
                TrackPublishOptions {
                    simulcast: false,
                    ..base(VideoCodec::H264)
                },
            ),
            (
                "vp9 sem simulcast, mantendo resolucao",
                TrackPublishOptions {
                    simulcast: false,
                    degradation_preference: Some(DegradationPreference::MaintainResolution),
                    ..base(VideoCodec::VP9)
                },
            ),
            // SVC seria o melhor dos dois mundos: uma passada de encoder e
            // mesmo assim uma escada para o SFU escolher. Uma sessao anterior
            // mediu L3T3_KEY entregando zero quadro ao espectador; as tres
            // variantes ficam aqui para que isso seja verificado e nao lembrado.
            (
                "vp9 svc L1T3 (so temporal)",
                TrackPublishOptions {
                    scalability_mode: Some("L1T3".to_owned()),
                    ..base(VideoCodec::VP9)
                },
            ),
            (
                "vp9 svc L2T3_KEY",
                TrackPublishOptions {
                    scalability_mode: Some("L2T3_KEY".to_owned()),
                    ..base(VideoCodec::VP9)
                },
            ),
            (
                "vp9 svc L3T3_KEY",
                TrackPublishOptions {
                    scalability_mode: Some("L3T3_KEY".to_owned()),
                    ..base(VideoCodec::VP9)
                },
            ),
            (
                "vp8 + escada padrao, 30 fps",
                TrackPublishOptions {
                    video_encoding: Some(VideoEncoding {
                        max_bitrate: 4_000_000,
                        max_framerate: 30.0,
                    }),
                    ..base(VideoCodec::VP8)
                },
            ),
            // A escada padrao de screenshare poe a camada de baixo em 3 fps: e
            // uma escada pensada para slides. Aqui se mede uma que continua
            // sendo video.
            (
                "vp8 + escada de 960x540@30",
                TrackPublishOptions {
                    simulcast_layers: Some(vec![VideoPreset::new(960, 540, 1_200_000, 30.0)]),
                    ..base(VideoCodec::VP8)
                },
            ),
            (
                "h264 + escada padrao",
                TrackPublishOptions {
                    ..base(VideoCodec::H264)
                },
            ),
            (
                "h264 sem simulcast, encoder de hardware",
                TrackPublishOptions {
                    simulcast: false,
                    video_encoder: livekit::options::VideoEncoderBackend::Hardware,
                    ..base(VideoCodec::H264)
                },
            ),
            (
                "h264 + escada padrao, encoder de hardware",
                TrackPublishOptions {
                    video_encoder: livekit::options::VideoEncoderBackend::Hardware,
                    ..base(VideoCodec::H264)
                },
            ),
            (
                "vp9 svc L1T3, 30 fps",
                TrackPublishOptions {
                    scalability_mode: Some("L1T3".to_owned()),
                    video_encoding: Some(VideoEncoding {
                        max_bitrate: 4_000_000,
                        max_framerate: 30.0,
                    }),
                    ..base(VideoCodec::VP9)
                },
            ),
        ];

        println!();
        println!(
            "encoders disponiveis: {:?}",
            livekit::options::VideoEncoderBackend::list_available()
                .into_iter()
                .collect::<Vec<_>>()
        );
        println!(
            "{:<42} {:>10} {:>12} {:>10} {:>8}",
            "variante", "espect./s", "resolucao", "kb/s", "cpu %"
        );
        for (label, options) in variants {
            let line = one_variant(label, options).await;
            println!("{line}");
        }
        println!();
    }

    async fn one_variant(label: &str, options: TrackPublishOptions) -> String {
        use livekit::track::{LocalTrack, LocalVideoTrack};
        use livekit::webrtc::prelude::RtcVideoSource;
        use livekit::webrtc::video_source::native::NativeVideoSource;
        use livekit::webrtc::video_source::VideoResolution;
        use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

        let room_name = format!("dvc-sweep-{}-{}", std::process::id(), label.len());
        let (_viewer, mut viewer_events) = Room::connect(
            &sfu_url(),
            &dev_token(&room_name, "espectador", false),
            RoomOptions::default(),
        )
        .await
        .expect("o espectador deve conectar; rode `just infra-up`");

        let frames = Arc::new(AtomicU64::new(0));
        let width = Arc::new(AtomicU32::new(0));
        let height = Arc::new(AtomicU32::new(0));
        let (counted, wide, tall) = (Arc::clone(&frames), Arc::clone(&width), Arc::clone(&height));
        tokio::spawn(async move {
            use futures_util::StreamExt as _;
            use livekit::webrtc::video_stream::native::NativeVideoStream;
            while let Some(event) = viewer_events.recv().await {
                if let livekit::RoomEvent::TrackSubscribed {
                    track: livekit::track::RemoteTrack::Video(video),
                    ..
                } = event
                {
                    let (counted, wide, tall) =
                        (Arc::clone(&counted), Arc::clone(&wide), Arc::clone(&tall));
                    tokio::spawn(async move {
                        let mut stream = NativeVideoStream::new(video.rtc_track());
                        while let Some(frame) = stream.next().await {
                            wide.store(frame.buffer.width(), Ordering::Relaxed);
                            tall.store(frame.buffer.height(), Ordering::Relaxed);
                            counted.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
            }
        });

        #[allow(clippy::field_reassign_with_default)]
        let room_options = {
            let mut o = RoomOptions::default();
            o.auto_subscribe = false;
            o.adaptive_stream = false;
            o.dynacast = true;
            o
        };
        let (room, _events) = Room::connect(
            &sfu_url(),
            &dev_token(&room_name, "medidor~pub", true),
            room_options,
        )
        .await
        .expect("o publicador deve conectar");

        let source = NativeVideoSource::new(
            VideoResolution {
                width: 1920,
                height: 1080,
            },
            true,
        );
        let track =
            LocalVideoTrack::create_video_track("screen", RtcVideoSource::Native(source.clone()));
        room.local_participant()
            .publish_track(LocalTrack::Video(track), options)
            .await
            .expect("deve publicar");

        let screen = capture::list_sources(&[])
            .into_iter()
            .find(|s| s.kind == SourceKind::Screen)
            .expect("deve existir uma tela");
        let capture = capture::start(
            SourceKind::Screen,
            screen.id.parse().expect("id numerico"),
            Size {
                width: 1920,
                height: 1080,
            },
            60,
            source,
            None,
            || {},
        )
        .expect("a captura deve iniciar");

        tokio::time::sleep(Duration::from_secs(4)).await;
        frames.store(0, Ordering::Relaxed);
        let started = Instant::now();
        let cpu_before = process_cpu_seconds();
        tokio::time::sleep(Duration::from_secs(12)).await;
        let elapsed = started.elapsed().as_secs_f64();
        // Em nucleos inteiros, e nao em porcentagem de uma maquina: "80 %" nao
        // diz nada sem saber quantos nucleos a maquina tem, e o que interessa e
        // quanto do orcamento de quem transmite o encoder come.
        let cpu = (process_cpu_seconds() - cpu_before) / elapsed * 100.0;

        let got = frames.load(Ordering::Relaxed);
        let (w, h) = (
            width.load(Ordering::Relaxed),
            height.load(Ordering::Relaxed),
        );
        let mut bytes = 0u64;
        if let Ok(report) = room.get_stats().await {
            for entry in &report.publisher_stats {
                if let RtcStats::OutboundRtp(outbound) = entry {
                    bytes += outbound.sent.bytes_sent;
                }
            }
        }
        capture.stop();
        let _ = room.close().await;

        format!(
            "{label:<42} {:>10.1} {:>12} {:>10.0} {cpu:>8.0}",
            got as f64 / elapsed,
            format!("{w}x{h}"),
            bytes as f64 * 8.0 / (elapsed + 4.0) / 1000.0
        )
    }

    /// CPU time this process has burned, in seconds, user plus kernel.
    ///
    /// The encoder is the single biggest cost the product imposes on whoever is
    /// sharing, and it is the one number that decides whether a preset is
    /// honest on a normal machine rather than on this one.
    fn process_cpu_seconds() -> f64 {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        if ok.is_err() {
            return 0.0;
        }
        let to_secs = |t: FILETIME| {
            ((u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)) as f64 / 1e7
        };
        to_secs(kernel) + to_secs(user)
    }

    /// RF-36: 1080p60 has to arrive as 1080p60, or the option is a lie.
    #[tokio::test]
    #[ignore]
    async fn measure_1080p60_end_to_end() {
        let (got, stats, produced, _) = measure(Preset::P1080p60, 20).await;

        assert!(
            produced >= 480,
            "a captura entregou {produced} quadros em 10 s; o teto e 600 e abaixo de 48 /s \
             a thread de captura nao esta dando conta do proprio relogio"
        );
        assert_eq!(
            (got.width, got.height),
            (1920, 1080),
            "o espectador recebeu {}x{}, e nao 1080p",
            got.width,
            got.height
        );
        assert!(
            got.frames >= 450,
            "o espectador recebeu {} quadros em 10 s (menos de 45 /s). \
             O encoder reportou {} fps e limitacao por '{}'",
            got.frames,
            stats.fps,
            stats.limited_by
        );
    }

    /// The same measurement at the preset most machines will actually sit on.
    #[tokio::test]
    #[ignore]
    async fn measure_1080p30_end_to_end() {
        let (got, _, _, _) = measure(Preset::P1080p30, 10).await;
        assert_eq!((got.width, got.height), (1920, 1080));
        assert!(
            got.frames >= 240,
            "o espectador recebeu {} quadros",
            got.frames
        );
    }

    /// The preset the panel tells a CPU-bound publisher to fall back to.
    ///
    /// Worth its own measurement: recommending a way out that nobody checked is
    /// how a workaround becomes a second bug report.
    #[tokio::test]
    #[ignore]
    async fn measure_720p60_end_to_end() {
        let (got, _, _, _) = measure(Preset::P720p60, 10).await;
        assert_eq!((got.width, got.height), (1280, 720));
        assert!(
            got.frames >= 450,
            "o espectador recebeu {} quadros em 10 s",
            got.frames
        );
    }

    /// The third leg of the version pair (ADR-0019).
    ///
    /// The other two are guarded already: `livekit-client` in
    /// `desktop/src/media/versions.test.ts`, and the server image in
    /// `crates/api/src/livekit.rs`. This SDK speaks the same signalling
    /// protocol, so it can drift away from the server exactly the same way —
    /// and when it does, only publishing breaks, which is the failure that cost
    /// a whole debugging session once already.
    #[test]
    fn the_rust_sdk_is_pinned_to_an_exact_version() {
        let manifest = include_str!("../Cargo.toml");
        let line = manifest
            .lines()
            .find(|line| line.starts_with("livekit ="))
            .expect("o Cargo.toml deve declarar o SDK do LiveKit");

        assert!(
            line.contains("version = \"="),
            "o livekit esta como {line}. Uma faixa deixa o SDK derivar para longe do \
             servidor e quebra so a publicacao. Ver ADR-0019."
        );
    }
}
