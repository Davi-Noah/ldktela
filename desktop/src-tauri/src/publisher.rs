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
use livekit::webrtc::stats::{OutboundRtpStats, RtcStats};
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::VideoResolution;
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

/// Three spatial layers, three temporal, with layer switching only on key
/// frames.
///
/// VP9 carries its ladder as SVC rather than as separate simulcast encodings, so
/// `simulcast` stays off and this carries the shape instead. Three spatial
/// layers means 1080/540/270, which is what makes a screen rendered small in the
/// grid cost the small layer (RF-32) — the whole egress argument depends on
/// this line.
const SCALABILITY_MODE: &str = "L3T3_KEY";

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
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct PublisherStats {
    pub bitrate_kbps: u32,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    /// True when libwebrtc chose a hardware encoder. Worth surfacing: it is the
    /// difference between a core of CPU and almost none, and until the move to
    /// the core it was not even observable.
    pub hardware_encoder: bool,
    /// Samples per channel captured so far, when sharing with audio.
    ///
    /// It is the only thing that tells a muted game apart from a broken capture:
    /// both sound like silence to everyone watching, and only one is our fault.
    pub audio_samples: Option<u64>,
}

pub struct Publisher {
    room: Arc<Room>,
    video: NativeVideoSource,
    audio: Option<NativeAudioSource>,
    track: LocalVideoTrack,
    /// Last (bytes_sent, instant) seen, so bitrate is a delta and not a total.
    last_sample: StdMutex<Option<(u64, Instant)>>,
}

impl Publisher {
    /// Connects and publishes an (initially empty) screen track.
    ///
    /// The track is published before any frame arrives on purpose: it is what
    /// produces the `track_published` webhook, and therefore `SHARE_START` and
    /// the "on air" clock every viewer reads (RF-34).
    pub async fn start(
        url: &str,
        token: &str,
        preset: Preset,
        with_audio: bool,
        on_disconnect: impl Fn(String) + Send + 'static,
    ) -> Result<Self, PublishError> {
        let ceiling = preset.ceiling();
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

        room.local_participant()
            .publish_track(
                LocalTrack::Video(track.clone()),
                TrackPublishOptions {
                    source: TrackSource::Screenshare,
                    video_codec: VideoCodec::VP9,
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
                    stream: "screen".to_owned(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| PublishError::Publish(error.to_string()))?;

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
            room.local_participant()
                .publish_track(
                    LocalTrack::Audio(track),
                    TrackPublishOptions {
                        source: TrackSource::ScreenshareAudio,
                        // Audio de jogo nao tem silencio para o DTX cortar, e o
                        // corte engole o ataque das notas.
                        dtx: false,
                        red: false,
                        stream: "screen".to_owned(),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|error| PublishError::Publish(error.to_string()))?;
            Some(source)
        } else {
            None
        };

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
            video,
            audio,
            track,
            last_sample: StdMutex::new(None),
        })
    }

    /// Bitrate, frame rate and the resolution actually being encoded.
    ///
    /// Summed across every spatial layer: with SVC the encoder produces several,
    /// and reporting only one would understate what the upload is costing. The
    /// resolution reported is the widest layer, which is what the top viewer
    /// sees.
    pub async fn stats(&self) -> PublisherStats {
        let Ok(report) = self.track.get_stats().await else {
            return PublisherStats::default();
        };

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
                ..Default::default()
            };
        };
        PublisherStats {
            bitrate_kbps,
            fps: widest.outbound.frames_per_second.round() as u32,
            width: widest.outbound.frame_width,
            height: widest.outbound.frame_height,
            hardware_encoder: widest.outbound.power_efficient_encoder,
            // Preenchido por quem tem a captura de audio em maos.
            audio_samples: None,
        }
    }

    pub fn video_sink(&self) -> NativeVideoSource {
        self.video.clone()
    }

    pub fn audio_sink(&self) -> Option<NativeAudioSource> {
        self.audio.clone()
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
