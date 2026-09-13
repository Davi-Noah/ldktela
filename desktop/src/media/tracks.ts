import {
  LocalAudioTrack,
  LocalVideoTrack,
  Track,
  VideoPreset,
  VideoQuality,
  createLocalScreenTracks,
} from 'livekit-client';
import type {
  LocalParticipant,
  RemoteTrackPublication,
  TrackPublishOptions,
  VideoSenderStats,
} from 'livekit-client';
import type { PublisherStats, QualityChoice } from '../store/media';

/**
 * The only module in the app that acquires media (ADR-0005). `contentHint`,
 * `degradationPreference`, the simulcast layers and the codec choice all live
 * here, because that is the part of the product most likely to change.
 */

/** ~6 Mbps is the figure the egress budget in RNF-05 is written against. */
export const LAYER_1080P60 = new VideoPreset({
  width: 1920,
  height: 1080,
  maxBitrate: 6_000_000,
  maxFramerate: 60,
  priority: 'high',
});

export const LAYER_720P30 = new VideoPreset({
  width: 1280,
  height: 720,
  maxBitrate: 1_800_000,
  maxFramerate: 30,
});

export const SCREEN_PUBLISH_OPTIONS: TrackPublishOptions = {
  source: Track.Source.ScreenShare,
  simulcast: true,
  videoCodec: 'vp9',
  backupCodec: { codec: 'h264' },
  // Motion over sharpness: this is gameplay, not a spreadsheet. It is also why we
  // override the SDK default of 'maintain-resolution' for screen share.
  degradationPreference: 'maintain-framerate',
  screenShareEncoding: LAYER_1080P60.encoding,
  // Only the H.264 backup uses these: VP9 carries its layers as SVC instead.
  screenShareSimulcastLayers: [LAYER_720P30],
  stream: 'screen',
};

export const SCREEN_AUDIO_PUBLISH_OPTIONS: TrackPublishOptions = {
  source: Track.Source.ScreenShareAudio,
  stream: 'screen',
  dtx: false,
  red: false,
  forceStereo: true,
};

export type CaptureSurface = 'monitor' | 'window';

export interface CaptureRequest {
  surface: CaptureSurface;
  audio: boolean;
}

export interface ScreenCapture {
  video: LocalVideoTrack;
  audio: LocalAudioTrack | null;
}

/** RF-14: window capture is video only until the per-process audio path exists. */
export function audioAvailableFor(surface: CaptureSurface): boolean {
  return surface === 'monitor';
}

export async function captureScreen(request: CaptureRequest): Promise<ScreenCapture> {
  const wantsAudio = request.audio && audioAvailableFor(request.surface);
  const tracks = await createLocalScreenTracks({
    video: { displaySurface: request.surface },
    audio: wantsAudio
      ? { echoCancellation: false, noiseSuppression: false, autoGainControl: false }
      : false,
    resolution: { width: 1920, height: 1080, frameRate: 60 },
    contentHint: 'motion',
    systemAudio: wantsAudio ? 'include' : 'exclude',
    selfBrowserSurface: 'exclude',
    surfaceSwitching: 'include',
  });

  let video: LocalVideoTrack | null = null;
  let audio: LocalAudioTrack | null = null;
  for (const track of tracks) {
    if (track instanceof LocalVideoTrack) {
      video = track;
    } else if (track instanceof LocalAudioTrack) {
      audio = track;
    }
  }
  if (video === null) {
    for (const track of tracks) {
      track.stop();
    }
    throw new Error('screen capture returned no video track');
  }
  return { video, audio };
}

export async function publishScreen(
  local: LocalParticipant,
  capture: ScreenCapture,
): Promise<void> {
  await local.publishTrack(capture.video, SCREEN_PUBLISH_OPTIONS);
  if (capture.audio !== null) {
    await local.publishTrack(capture.audio, SCREEN_AUDIO_PUBLISH_OPTIONS);
  }
}

export async function unpublishScreen(
  local: LocalParticipant,
  capture: ScreenCapture,
): Promise<void> {
  await local.unpublishTrack(capture.video, true);
  if (capture.audio !== null) {
    await local.unpublishTrack(capture.audio, true);
  }
}

export function stopCapture(capture: ScreenCapture): void {
  capture.video.stop();
  capture.audio?.stop();
}

/**
 * RF-19. `auto` leaves adaptiveStream in charge; the pinned choices cap the layer
 * the SFU is allowed to send. adaptiveStream can still go below a pinned layer
 * when the window is small — RF-16 outranks RF-19 on purpose.
 */
export function applyQuality(publication: RemoteTrackPublication, choice: QualityChoice): void {
  switch (choice) {
    case 'auto':
      publication.setVideoQuality(VideoQuality.HIGH);
      return;
    case 'high':
      publication.setVideoDimensions({ width: LAYER_1080P60.width, height: LAYER_1080P60.height });
      return;
    case 'low':
      publication.setVideoQuality(VideoQuality.MEDIUM);
      return;
  }
}

export interface SenderSample {
  bytesSent: number;
  timestamp: number;
}

export interface StatsReading {
  stats: PublisherStats;
  sample: SenderSample;
}

/**
 * Sampled on an interval by the caller, never in the media path. Bitrate is the
 * delta between two samples across every simulcast layer; resolution and fps come
 * from the widest layer actually being encoded.
 */
export function readSenderStats(
  layers: VideoSenderStats[],
  previous: SenderSample | null,
): StatsReading {
  let bytesSent = 0;
  let timestamp = 0;
  let widest: VideoSenderStats | null = null;
  for (const layer of layers) {
    bytesSent += layer.bytesSent ?? 0;
    timestamp = Math.max(timestamp, layer.timestamp);
    if (widest === null || layer.frameWidth > widest.frameWidth) {
      widest = layer;
    }
  }
  const sample: SenderSample = { bytesSent, timestamp };
  const elapsedMs = previous === null ? 0 : timestamp - previous.timestamp;
  const bitrateKbps =
    previous === null || elapsedMs <= 0
      ? 0
      : Math.round(((bytesSent - previous.bytesSent) * 8) / elapsedMs);
  return {
    sample,
    stats: {
      bitrateKbps: Math.max(0, bitrateKbps),
      fps: Math.round(widest?.framesPerSecond ?? 0),
      width: widest?.frameWidth ?? 0,
      height: widest?.frameHeight ?? 0,
    },
  };
}
