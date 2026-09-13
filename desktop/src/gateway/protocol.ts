import type { ClientInfo } from '../api/types/ClientInfo';
import type { DispatchEvent } from '../api/types/DispatchEvent';
import type { Hello } from '../api/types/Hello';
import type { Identify } from '../api/types/Identify';
import type { Resume } from '../api/types/Resume';

export const OPCODE = {
  DISPATCH: 0,
  HELLO: 1,
  IDENTIFY: 2,
  RESUME: 3,
  HEARTBEAT: 4,
  HEARTBEAT_ACK: 5,
  INVALID_SESSION: 6,
  RECONNECT: 7,
} as const;

export const CLOSE_AUTH_FAILED = 4001;
export const CLOSE_MALFORMED = 4002;
export const CLOSE_IDENTIFY_TIMEOUT = 4003;
export const CLOSE_SESSION_TAKEN = 4004;
export const CLOSE_RATE_LIMITED = 4008;
export const CLOSE_OUTDATED_CLIENT = 4010;
/** Client-initiated close when no HEARTBEAT_ACK arrives (websocket.md §3.2). */
export const CLOSE_ZOMBIE = 4900;

const DISPATCH_NAMES: ReadonlySet<string> = new Set<DispatchEvent['t']>([
  'READY',
  'RESUMED',
  'ROOM_JOIN',
  'ROOM_LEAVE',
  'ROOM_PARTICIPANT_ADD',
  'ROOM_PARTICIPANT_REMOVE',
  'SHARE_START',
  'SHARE_STOP',
]);

export type ServerFrame =
  | { kind: 'hello'; hello: Hello }
  | { kind: 'dispatch'; seq: number; event: DispatchEvent }
  | { kind: 'heartbeat_ack' }
  | { kind: 'invalid_session'; resumable: boolean }
  | { kind: 'reconnect' };

/**
 * Returns `null` for anything that is not a frame we know how to act on. A frame
 * we cannot read is dropped rather than fatal: §8 of the protocol requires the
 * client to ignore what it does not understand.
 */
export function parseServerFrame(raw: unknown): ServerFrame | null {
  if (typeof raw !== 'string') {
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch {
    return null;
  }
  const frame = asRecord(parsed);
  if (frame === null || typeof frame.op !== 'number') {
    return null;
  }
  switch (frame.op) {
    case OPCODE.HELLO:
      return readHello(frame.d);
    case OPCODE.DISPATCH:
      return readDispatch(frame);
    case OPCODE.HEARTBEAT_ACK:
      return { kind: 'heartbeat_ack' };
    case OPCODE.INVALID_SESSION: {
      const payload = asRecord(frame.d);
      return { kind: 'invalid_session', resumable: payload?.resumable === true };
    }
    case OPCODE.RECONNECT:
      return { kind: 'reconnect' };
    default:
      return null;
  }
}

function readHello(raw: unknown): ServerFrame | null {
  const payload = asRecord(raw);
  if (payload === null) {
    return null;
  }
  const interval = payload.heartbeat_interval_ms;
  const ttl = payload.session_ttl_ms;
  if (!isPositiveNumber(interval)) {
    return null;
  }
  return {
    kind: 'hello',
    hello: {
      heartbeat_interval_ms: interval,
      session_ttl_ms: isPositiveNumber(ttl) ? ttl : 0,
    },
  };
}

function readDispatch(frame: Record<string, unknown>): ServerFrame | null {
  const seq = frame.s;
  const name = frame.t;
  const data = frame.d;
  if (typeof seq !== 'number' || typeof name !== 'string' || !DISPATCH_NAMES.has(name)) {
    return null;
  }
  if (asRecord(data) === null) {
    return null;
  }
  // Only the discriminant is validated. The payload comes from our own server and
  // its shape is pinned by the generated types on both sides; re-validating every
  // field here would duplicate the Rust contract without catching anything real.
  const event = { t: name, d: data } as DispatchEvent;
  return { kind: 'dispatch', seq, event };
}

export function encodeIdentify(token: string, client: ClientInfo): string {
  const payload: Identify = { token, client };
  return JSON.stringify({ op: OPCODE.IDENTIFY, d: payload });
}

export function encodeResume(token: string, sessionId: string, lastSeq: number): string {
  const payload: Resume = { token, session_id: sessionId, last_seq: lastSeq };
  return JSON.stringify({ op: OPCODE.RESUME, d: payload });
}

export function encodeHeartbeat(): string {
  return JSON.stringify({ op: OPCODE.HEARTBEAT });
}

function isPositiveNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null;
}
