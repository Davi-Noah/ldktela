import { RemoteAudioTrack, RemoteVideoTrack, Room, RoomEvent, Track } from 'livekit-client';
import type { RemoteParticipant, RemoteTrackPublication } from 'livekit-client';
import { type ApiClient, ApiError } from '../api/client';
import { describeError, log } from '../log';
import type { Snowflake } from '../api/types/Snowflake';
import { STATS_SAMPLE_INTERVAL_MS } from '../config';
import { backoffDelayMs } from '../gateway/backoff';
import { type QualityChoice, useMediaStore } from '../store/media';
import type { CaptureRequest, ScreenCapture, SenderSample } from './tracks';
import {
  applyQuality,
  captureScreen,
  publishScreen,
  readSenderStats,
  stopCapture,
  unpublishScreen,
} from './tracks';

/**
 * Owns the LiveKit room. Joining is never a user action: the gateway says which
 * Discord voice channel we are in and this follows it (ADR-0011).
 */
export class MediaSession {
  private readonly api: ApiClient;
  private room: Room | null = null;
  private channelId: Snowflake | null = null;
  private canPublish = false;
  private capture: ScreenCapture | null = null;
  private videoElement: HTMLVideoElement | null = null;
  private audioElement: HTMLAudioElement | null = null;
  private activeVideo: RemoteVideoTrack | null = null;
  private activeAudio: RemoteAudioTrack | null = null;
  private activePublication: RemoteTrackPublication | null = null;
  private statsTimer: ReturnType<typeof setInterval> | null = null;
  private lastSample: SenderSample | null = null;
  private rejoinTimer: ReturnType<typeof setTimeout> | null = null;
  private rejoinAttempt = 0;

  constructor(api: ApiClient) {
    this.api = api;
  }

  /** The video element is created once and outlives every track (CLAUDE.md §7). */
  registerVideoElement(element: HTMLVideoElement | null): void {
    if (this.videoElement !== null && this.activeVideo !== null) {
      this.activeVideo.detach(this.videoElement);
    }
    this.videoElement = element;
    if (element !== null && this.activeVideo !== null) {
      this.activeVideo.attach(element);
    }
  }

  registerAudioElement(element: HTMLAudioElement | null): void {
    if (this.audioElement !== null && this.activeAudio !== null) {
      this.activeAudio.detach(this.audioElement);
    }
    this.audioElement = element;
    if (element !== null && this.activeAudio !== null) {
      this.activeAudio.attach(element);
    }
  }

  async follow(channelId: Snowflake | null): Promise<void> {
    if (channelId === this.channelId) {
      return;
    }
    await this.leave();
    this.channelId = channelId;
    if (channelId !== null) {
      await this.connect(false);
    }
  }

  async leave(): Promise<void> {
    this.clearRejoin();
    this.stopStatsSampling();
    this.channelId = null;
    this.canPublish = false;
    const room = this.room;
    this.room = null;
    this.detachRemote();
    if (this.capture !== null) {
      stopCapture(this.capture);
      this.capture = null;
    }
    if (room !== null) {
      room.removeAllListeners();
      await room.disconnect(true);
    }
    useMediaStore.getState().reset();
  }

  async startShare(request: CaptureRequest): Promise<void> {
    const store = useMediaStore.getState();
    if (this.channelId === null || this.capture !== null) {
      return;
    }
    store.setError(null);
    store.setStarting(true);
    let capture: ScreenCapture;
    try {
      // The OS picker runs first: it is the slow part, and a user who cancels it
      // must not cost an admission slot.
      capture = await captureScreen(request);
    } catch (error) {
      store.setStarting(false);
      store.setError(captureMessage(error));
      return;
    }

    try {
      if (!this.canPublish) {
        log.debug('compartilhamento: reconectando com token de publicação');
        await this.connect(true);
      }
      const room = this.room;
      if (room === null) {
        throw new Error('a sala não está conectada');
      }
      log.debug('compartilhamento: publicando trilhas no LiveKit');
      await publishScreen(room.localParticipant, capture);
      log.info('compartilhamento: no ar');
      this.capture = capture;
      capture.video.mediaStreamTrack.addEventListener(
        'ended',
        () => {
          // The user pressed the browser's own stop-sharing control.
          void this.stopShare();
        },
        { once: true },
      );
      store.setPublishing(true, capture.audio !== null);
      this.startStatsSampling();
      this.syncViewers();
    } catch (error) {
      log.error('compartilhamento: publicação falhou', error);
      stopCapture(capture);
      store.setStarting(false);
      store.setError(publishMessage(error));
    }
  }

  async stopShare(): Promise<void> {
    const capture = this.capture;
    this.capture = null;
    this.stopStatsSampling();
    useMediaStore.getState().setPublishing(false, false);
    if (capture === null) {
      return;
    }
    const room = this.room;
    if (room !== null) {
      await unpublishScreen(room.localParticipant, capture);
    }
    stopCapture(capture);
  }

  setQuality(choice: QualityChoice): void {
    useMediaStore.getState().setQuality(choice);
    if (this.activePublication !== null) {
      applyQuality(this.activePublication, choice);
    }
  }

  private async connect(publish: boolean): Promise<void> {
    const channelId = this.channelId;
    if (channelId === null) {
      return;
    }
    const store = useMediaStore.getState();
    store.setConnection('connecting');

    const previous = this.room;
    this.room = null;
    this.detachRemote();
    if (previous !== null) {
      previous.removeAllListeners();
      await previous.disconnect(false);
    }

    log.debug('sala: pedindo token', { canal: channelId, publicar: publish });
    let credentials;
    try {
      credentials = await this.api.roomToken(channelId, publish);
    } catch (error) {
      log.error('sala: o servidor recusou o token', error, { canal: channelId });
      store.setConnection('failed');
      throw error;
    }
    log.debug('sala: token recebido', { url: credentials.url, sala: credentials.room });
    const room = new Room({
      // RF-16: both are mandatory. A viewer who is not looking receives no layer.
      adaptiveStream: true,
      dynacast: true,
      stopLocalTrackOnUnpublish: false,
    });
    this.wire(room);
    this.room = room;
    this.canPublish = publish;
    try {
      await room.connect(credentials.url, credentials.token);
    } catch (error) {
      log.error('sala: LiveKit recusou a conexão', error, { url: credentials.url });
      this.room = null;
      this.canPublish = false;
      store.setConnection('failed');
      throw error;
    }
    log.info('sala: conectado ao LiveKit', { sala: credentials.room, publicar: publish });
    store.setConnection('connected');
    this.adoptExistingTracks(room);
    this.syncViewers();
  }

  private wire(room: Room): void {
    room.on(RoomEvent.TrackSubscribed, (_track, publication, participant) => {
      this.adoptPublication(publication, participant);
    });
    room.on(RoomEvent.TrackUnsubscribed, (_track, publication) => {
      this.dropPublication(publication);
    });
    room.on(RoomEvent.ParticipantConnected, () => {
      this.syncViewers();
    });
    room.on(RoomEvent.ParticipantDisconnected, () => {
      this.syncViewers();
    });
    room.on(RoomEvent.Reconnecting, () => {
      useMediaStore.getState().setConnection('reconnecting');
    });
    room.on(RoomEvent.Reconnected, () => {
      useMediaStore.getState().setConnection('connected');
      this.syncViewers();
    });
    room.on(RoomEvent.Disconnected, () => {
      if (this.room === room) {
        this.scheduleRejoin();
      }
    });
  }

  private adoptExistingTracks(room: Room): void {
    for (const participant of room.remoteParticipants.values()) {
      for (const publication of participant.trackPublications.values()) {
        this.adoptPublication(publication, participant);
      }
    }
  }

  private adoptPublication(
    publication: RemoteTrackPublication,
    participant: RemoteParticipant,
  ): void {
    const track = publication.track;
    if (publication.source === Track.Source.ScreenShare && track instanceof RemoteVideoTrack) {
      this.activeVideo = track;
      this.activePublication = publication;
      if (this.videoElement !== null) {
        track.attach(this.videoElement);
      }
      applyQuality(publication, useMediaStore.getState().quality);
      useMediaStore.getState().setWatching(participant.identity);
      this.syncViewers();
      return;
    }
    if (publication.source === Track.Source.ScreenShareAudio && track instanceof RemoteAudioTrack) {
      this.activeAudio = track;
      if (this.audioElement !== null) {
        track.attach(this.audioElement);
      }
    }
  }

  private dropPublication(publication: RemoteTrackPublication): void {
    if (publication === this.activePublication) {
      this.detachVideo();
      useMediaStore.getState().setWatching(null);
      this.syncViewers();
      return;
    }
    if (publication.source === Track.Source.ScreenShareAudio) {
      this.detachAudio();
    }
  }

  private detachRemote(): void {
    this.detachVideo();
    this.detachAudio();
    useMediaStore.getState().setWatching(null);
  }

  private detachVideo(): void {
    if (this.activeVideo !== null && this.videoElement !== null) {
      this.activeVideo.detach(this.videoElement);
    }
    this.activeVideo = null;
    this.activePublication = null;
  }

  private detachAudio(): void {
    if (this.activeAudio !== null && this.audioElement !== null) {
      this.activeAudio.detach(this.audioElement);
    }
    this.activeAudio = null;
  }

  private syncViewers(): void {
    const room = this.room;
    if (room === null) {
      useMediaStore.getState().setViewerIds([]);
      return;
    }
    const viewers: string[] = [];
    for (const participant of room.remoteParticipants.values()) {
      if (participant.getTrackPublication(Track.Source.ScreenShare) === undefined) {
        viewers.push(participant.identity);
      }
    }
    useMediaStore.getState().setViewerIds(viewers);
  }

  private startStatsSampling(): void {
    this.stopStatsSampling();
    this.lastSample = null;
    this.statsTimer = setInterval(() => {
      void this.sampleStats();
    }, STATS_SAMPLE_INTERVAL_MS);
  }

  private async sampleStats(): Promise<void> {
    const capture = this.capture;
    if (capture === null) {
      return;
    }
    try {
      const layers = await capture.video.getSenderStats();
      const reading = readSenderStats(layers, this.lastSample);
      this.lastSample = reading.sample;
      useMediaStore.getState().setStats(reading.stats);
    } catch {
      // A stats read that fails is not worth surfacing; the next tick tries again.
    }
  }

  private stopStatsSampling(): void {
    if (this.statsTimer !== null) {
      clearInterval(this.statsTimer);
      this.statsTimer = null;
    }
    this.lastSample = null;
  }

  private scheduleRejoin(): void {
    if (this.channelId === null || this.rejoinTimer !== null) {
      return;
    }
    const store = useMediaStore.getState();
    store.setConnection('reconnecting');
    const wantsPublish = this.canPublish;
    const delay = backoffDelayMs(this.rejoinAttempt);
    this.rejoinAttempt += 1;
    this.rejoinTimer = setTimeout(() => {
      this.rejoinTimer = null;
      // A fresh token, not the old one: the media token can simply have expired.
      void this.connect(wantsPublish).then(
        () => {
          this.rejoinAttempt = 0;
        },
        () => {
          this.scheduleRejoin();
        },
      );
    }, delay);
  }

  private clearRejoin(): void {
    if (this.rejoinTimer !== null) {
      clearTimeout(this.rejoinTimer);
      this.rejoinTimer = null;
    }
    this.rejoinAttempt = 0;
  }
}

function captureMessage(error: unknown): string | null {
  if (error instanceof DOMException && error.name === 'NotAllowedError') {
    // Cancelling the picker is not an error worth showing.
    return null;
  }
  return `Não foi possível capturar a tela. (${describeError(error)})`;
}

function publishMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 409) {
      return 'A sala já está com o número máximo de telas compartilhadas.';
    }
    if (error.status === 403 || error.status === 404) {
      return 'Você não pode compartilhar neste canal.';
    }
    return error.message;
  }
  return `Não foi possível iniciar o compartilhamento. (${describeError(error)})`;
}
