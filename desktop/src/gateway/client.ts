import type { ClientInfo } from '../api/types/ClientInfo';
import type { DispatchEvent } from '../api/types/DispatchEvent';
import { SESSION_STABLE_MS, backoffDelayMs, heartbeatDelayMs } from './backoff';
import {
  CLOSE_AUTH_FAILED,
  CLOSE_IDENTIFY_TIMEOUT,
  CLOSE_MALFORMED,
  CLOSE_OUTDATED_CLIENT,
  CLOSE_SESSION_TAKEN,
  CLOSE_ZOMBIE,
  encodeHeartbeat,
  encodeIdentify,
  encodeResume,
  parseServerFrame,
} from './protocol';

export type GatewayStatus =
  'idle' | 'connecting' | 'identifying' | 'resuming' | 'ready' | 'reconnecting' | 'closed';

/** Reasons the client stops on its own instead of reconnecting. */
export type GatewayFatal = 'auth' | 'outdated_client';

/** The slice of `WebSocket` the client uses, so tests can hand it a fake. */
export interface GatewaySocket {
  send(data: string): void;
  close(code?: number, reason?: string): void;
  onopen: ((event: Event) => void) | null;
  onmessage: ((event: MessageEvent) => void) | null;
  onclose: ((event: CloseEvent) => void) | null;
  onerror: ((event: Event) => void) | null;
}

export interface GatewayClientOptions {
  url: string;
  client: ClientInfo;
  getAccessToken: () => string | null;
  onEvent: (event: DispatchEvent) => void;
  onStatus: (status: GatewayStatus) => void;
  onFatal: (reason: GatewayFatal) => void;
  createSocket?: (url: string) => GatewaySocket;
  random?: () => number;
  now?: () => number;
}

/** Two unanswered heartbeats in a row means the connection is a zombie. */
const MAX_MISSED_ACKS = 2;

export class GatewayClient {
  private readonly options: GatewayClientOptions;
  private readonly random: () => number;
  private readonly now: () => number;

  private socket: GatewaySocket | null = null;
  private status: GatewayStatus = 'idle';
  private sessionId: string | null = null;
  private lastSeq = 0;
  private attempt = 0;
  private stopped = true;
  private missedAcks = 0;
  private heartbeatIntervalMs = 30_000;
  private readyAt: number | null = null;
  /** Set by opcode 7: the server asked for a reconnect, so skip the backoff once. */
  private reconnectNow = false;
  private heartbeatTimer: ReturnType<typeof setTimeout> | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(options: GatewayClientOptions) {
    this.options = options;
    this.random = options.random ?? Math.random;
    this.now = options.now ?? (() => Date.now());
  }

  getStatus(): GatewayStatus {
    return this.status;
  }

  start(): void {
    if (!this.stopped) {
      return;
    }
    this.stopped = false;
    this.attempt = 0;
    this.open();
  }

  stop(): void {
    this.stopped = true;
    this.clearHeartbeat();
    this.clearRetry();
    this.sessionId = null;
    this.lastSeq = 0;
    this.readyAt = null;
    const socket = this.socket;
    this.socket = null;
    if (socket !== null) {
      detach(socket);
      socket.close(1000, 'client stop');
    }
    this.setStatus('closed');
  }

  private open(): void {
    const token = this.options.getAccessToken();
    if (token === null) {
      this.stopped = true;
      this.setStatus('closed');
      this.options.onFatal('auth');
      return;
    }
    this.missedAcks = 0;
    this.setStatus('connecting');
    const socket = (this.options.createSocket ?? defaultSocket)(this.options.url);
    this.socket = socket;
    socket.onopen = null;
    socket.onerror = null;
    socket.onmessage = (event: MessageEvent) => {
      this.handleFrame(event.data);
    };
    socket.onclose = (event: CloseEvent) => {
      this.handleClose(event.code);
    };
  }

  private handleFrame(data: unknown): void {
    const frame = parseServerFrame(data);
    if (frame === null) {
      return;
    }
    switch (frame.kind) {
      case 'hello':
        this.handleHello(frame.hello.heartbeat_interval_ms);
        return;
      case 'dispatch':
        this.handleDispatch(frame.seq, frame.event);
        return;
      case 'heartbeat_ack':
        this.missedAcks = 0;
        return;
      case 'invalid_session':
        // The buffer no longer covers us. Identify fresh; the READY carries the
        // whole room state, so there is no gap for REST to recover.
        this.sessionId = null;
        this.lastSeq = 0;
        this.sendIdentify();
        return;
      case 'reconnect':
        this.reconnectNow = true;
        this.socket?.close(1000, 'server requested reconnect');
        return;
    }
  }

  private handleHello(intervalMs: number): void {
    this.heartbeatIntervalMs = intervalMs;
    this.missedAcks = 0;
    if (this.sessionId !== null) {
      const token = this.options.getAccessToken();
      if (token !== null) {
        this.send(encodeResume(token, this.sessionId, this.lastSeq));
        this.setStatus('resuming');
      }
    } else {
      this.sendIdentify();
    }
    this.scheduleHeartbeat();
  }

  private sendIdentify(): void {
    const token = this.options.getAccessToken();
    if (token === null) {
      this.socket?.close(1000, 'no access token');
      return;
    }
    this.send(encodeIdentify(token, this.options.client));
    this.setStatus('identifying');
  }

  private handleDispatch(seq: number, event: DispatchEvent): void {
    this.lastSeq = seq;
    if (event.t === 'READY') {
      this.sessionId = event.d.session_id;
      this.heartbeatIntervalMs = event.d.heartbeat_interval_ms;
      this.readyAt = this.now();
      this.setStatus('ready');
    } else if (event.t === 'RESUMED') {
      this.readyAt = this.now();
      this.setStatus('ready');
    }
    this.options.onEvent(event);
  }

  private handleClose(code: number): void {
    this.clearHeartbeat();
    const socket = this.socket;
    this.socket = null;
    if (socket !== null) {
      detach(socket);
    }
    if (this.stopped) {
      this.setStatus('closed');
      return;
    }

    if (code === CLOSE_OUTDATED_CLIENT) {
      this.stopped = true;
      this.setStatus('closed');
      this.options.onFatal('outdated_client');
      return;
    }
    if (code === CLOSE_AUTH_FAILED) {
      // The caller decides whether a token refresh can save the session; it calls
      // start() again if it can.
      this.stopped = true;
      this.sessionId = null;
      this.lastSeq = 0;
      this.setStatus('closed');
      this.options.onFatal('auth');
      return;
    }
    if (
      code === CLOSE_MALFORMED ||
      code === CLOSE_IDENTIFY_TIMEOUT ||
      code === CLOSE_SESSION_TAKEN
    ) {
      this.sessionId = null;
      this.lastSeq = 0;
    }

    if (this.readyAt !== null && this.now() - this.readyAt > SESSION_STABLE_MS) {
      this.attempt = 0;
    }
    this.readyAt = null;

    let delay = 0;
    if (this.reconnectNow) {
      this.reconnectNow = false;
    } else {
      delay = backoffDelayMs(this.attempt, this.random);
      this.attempt += 1;
    }
    this.setStatus('reconnecting');
    this.clearRetry();
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      if (!this.stopped) {
        this.open();
      }
    }, delay);
  }

  private scheduleHeartbeat(): void {
    this.clearHeartbeat();
    this.heartbeatTimer = setTimeout(
      () => {
        this.heartbeatTimer = null;
        this.beat();
      },
      heartbeatDelayMs(this.heartbeatIntervalMs, this.random),
    );
  }

  private beat(): void {
    const socket = this.socket;
    if (socket === null) {
      return;
    }
    if (this.missedAcks >= MAX_MISSED_ACKS) {
      socket.close(CLOSE_ZOMBIE, 'no heartbeat ack');
      return;
    }
    this.missedAcks += 1;
    this.send(encodeHeartbeat());
    this.scheduleHeartbeat();
  }

  private send(data: string): void {
    try {
      this.socket?.send(data);
    } catch {
      // A socket that refuses a write is already closing; onclose drives recovery.
    }
  }

  private setStatus(status: GatewayStatus): void {
    if (this.status === status) {
      return;
    }
    this.status = status;
    this.options.onStatus(status);
  }

  private clearHeartbeat(): void {
    if (this.heartbeatTimer !== null) {
      clearTimeout(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  private clearRetry(): void {
    if (this.retryTimer !== null) {
      clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
  }
}

function detach(socket: GatewaySocket): void {
  socket.onopen = null;
  socket.onmessage = null;
  socket.onclose = null;
  socket.onerror = null;
}

function defaultSocket(url: string): GatewaySocket {
  return new WebSocket(url);
}
