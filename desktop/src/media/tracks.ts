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
import type { PublishPreset, PublisherStats, QualityChoice } from '../store/media';

/**
 * The only module in the app that acquires media (ADR-0005). `contentHint`,
 * `degradationPreference`, the simulcast layers and the codec choice all live
 * here, because that is the part of the product most likely to change.
 */

/**
 * The ladder each publisher preset produces (RF-36).
 *
 * The publisher picks resolution and frame rate because the publisher pays for
 * the encode; the viewer picks among the layers that result (ADR-0023). Two
 * layers, not four: a four-way ladder doubles the encode cost of one person to
 * give options to everyone else, and the encode already costs ~1 core at
 * 1080p60 (RESULTS.md, RNF-04).
 */
interface Ladder {
  high: VideoPreset;
  low: VideoPreset;
}

/** ~6 Mbps at 1080p60 is the figure the egress budget in RNF-05 is written against. */
const LADDERS: Record<PublishPreset, Ladder> = {
  '1080p60': {
    high: new VideoPreset({
      width: 1920,
      height: 1080,
      maxBitrate: 6_000_000,
      maxFramerate: 60,
      priority: 'high',
    }),
    low: new VideoPreset({ width: 960, height: 540, maxBitrate: 1_200_000, maxFramerate: 30 }),
  },
  '1080p30': {
    high: new VideoPreset({
      width: 1920,
      height: 1080,
      maxBitrate: 4_000_000,
      maxFramerate: 30,
      priority: 'high',
    }),
    low: new VideoPreset({ width: 960, height: 540, maxBitrate: 1_000_000, maxFramerate: 30 }),
  },
  '720p60': {
    high: new VideoPreset({
      width: 1280,
      height: 720,
      maxBitrate: 3_000_000,
      maxFramerate: 60,
      priority: 'high',
    }),
    low: new VideoPreset({ width: 640, height: 360, maxBitrate: 700_000, maxFramerate: 30 }),
  },
  '720p30': {
    high: new VideoPreset({
      width: 1280,
      height: 720,
      maxBitrate: 1_800_000,
      maxFramerate: 30,
      priority: 'high',
    }),
    low: new VideoPreset({ width: 640, height: 360, maxBitrate: 500_000, maxFramerate: 30 }),
  },
};

export function ladderFor(preset: PublishPreset): Ladder {
  return LADDERS[preset];
}

export function screenPublishOptions(preset: PublishPreset): TrackPublishOptions {
  const ladder = LADDERS[preset];
  return {
    source: Track.Source.ScreenShare,
    simulcast: true,
    videoCodec: 'vp9',
    backupCodec: { codec: 'h264' },
    // Motion over sharpness: this is gameplay, not a spreadsheet. It is also why
    // we override the SDK default of 'maintain-resolution' for screen share.
    degradationPreference: 'maintain-framerate',
    screenShareEncoding: ladder.high.encoding,
    // Only the H.264 backup uses these: VP9 carries its layers as SVC instead.
    screenShareSimulcastLayers: [ladder.low],
    stream: 'screen',
  };
}

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
  preset: PublishPreset;
}

export interface ScreenCapture {
  video: LocalVideoTrack;
  audio: LocalAudioTrack | null;
  /** Remembered so republishing with a new preset can reuse the same choice. */
  surface: CaptureSurface;
}

/** RF-14: window capture is video only until the per-process audio path exists. */
export function audioAvailableFor(surface: CaptureSurface): boolean {
  return surface === 'monitor';
}

export async function captureScreen(request: CaptureRequest): Promise<ScreenCapture> {
  const wantsAudio = request.audio && audioAvailableFor(request.surface);
  const { high } = LADDERS[request.preset];
  const tracks = await createLocalScreenTracks({
    video: { displaySurface: request.surface },
    audio: wantsAudio
      ? { echoCancellation: false, noiseSuppression: false, autoGainControl: false }
      : false,
    resolution: { width: high.width, height: high.height, frameRate: high.encoding.maxFramerate },
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
  return { video, audio, surface: request.surface };
}

export async function publishScreen(
  local: LocalParticipant,
  capture: ScreenCapture,
  preset: PublishPreset,
): Promise<void> {
  await local.publishTrack(capture.video, screenPublishOptions(preset));
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
 * RF-19, and the viewer half of ADR-0023.
 *
 * `auto` leaves `adaptiveStream` in charge, which is what keeps a grid of N
 * screens from costing N full streams (RF-32): a video rendered small gets the
 * small layer on its own. `high` overrides that heuristic by asking for the
 * published dimensions, which is the only way to get the top layer into a
 * thumbnail-sized element. `low` caps at the bottom layer.
 *
 * There is no frame-rate choice here on purpose: frame rate belongs to the
 * publisher, and a selector offering one would be offering something nobody is
 * sending.
 */
export function applyQuality(publication: RemoteTrackPublication, choice: QualityChoice): void {
  switch (choice) {
    case 'auto':
      publication.setVideoQuality(VideoQuality.HIGH);
      return;
    case 'high': {
      const published = publication.dimensions;
      if (published !== undefined) {
        publication.setVideoDimensions(published);
      }
      publication.setVideoQuality(VideoQuality.HIGH);
      return;
    }
    case 'low':
      publication.setVideoQuality(VideoQuality.LOW);
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
