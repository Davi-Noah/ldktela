import type { DispatchEvent } from '../api/types/DispatchEvent';
import type { GatewayFatal, GatewaySocket, GatewayStatus } from './client';
import { GatewayClient } from './client';
import { CLOSE_ZOMBIE, OPCODE } from './protocol';

class FakeSocket implements GatewaySocket {
  sent: string[] = [];
  closedWith: number | null = null;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;

  send(data: string): void {
    this.sent.push(data);
  }

  close(code = 1000): void {
    if (this.closedWith !== null) {
      return;
    }
    this.closedWith = code;
    this.onclose?.(new CloseEvent('close', { code }));
  }

  serverClose(code: number): void {
    this.closedWith = code;
    this.onclose?.(new CloseEvent('close', { code }));
  }

  deliver(frame: unknown): void {
    this.onmessage?.(new MessageEvent('message', { data: JSON.stringify(frame) }));
  }

  opcodes(): number[] {
    return this.sent.map((raw) => (JSON.parse(raw) as { op: number }).op);
  }

  payload(index: number): unknown {
    const raw = this.sent[index];
    return raw === undefined ? null : (JSON.parse(raw) as { d?: unknown }).d;
  }
}

const HELLO = { op: OPCODE.HELLO, d: { heartbeat_interval_ms: 30_000, session_ttl_ms: 90_000 } };

function ready(seq: number, sessionId: string) {
  return {
    op: OPCODE.DISPATCH,
    s: seq,
    t: 'READY',
    d: {
      session_id: sessionId,
      user: {
        id: 'u1',
        discord_user_id: '1',
        username: 'ana',
        display_name: null,
        avatar_url: null,
        created_at: '2026-01-01T00:00:00Z',
      },
      heartbeat_interval_ms: 30_000,
    },
  };
}

interface Harness {
  client: GatewayClient;
  sockets: FakeSocket[];
  statuses: GatewayStatus[];
  events: DispatchEvent[];
  fatals: GatewayFatal[];
  latest: () => FakeSocket;
}

function harness(token: string | null = 'access-token'): Harness {
  const sockets: FakeSocket[] = [];
  const statuses: GatewayStatus[] = [];
  const events: DispatchEvent[] = [];
  const fatals: GatewayFatal[] = [];
  const client = new GatewayClient({
    url: 'ws://localhost/gateway?v=1',
    client: { version: '0.1.0', os: 'windows' },
    getAccessToken: () => token,
    onEvent: (event) => events.push(event),
    onStatus: (status) => statuses.push(status),
    onFatal: (reason) => fatals.push(reason),
    createSocket: () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket;
    },
    random: () => 0.5,
    now: () => Date.now(),
  });
  return {
    client,
    sockets,
    statuses,
    events,
    fatals,
    latest: () => {
      const socket = sockets[sockets.length - 1];
      if (socket === undefined) {
        throw new Error('no socket was created');
      }
      return socket;
    },
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('handshake', () => {
  it('identifies after HELLO and reaches ready on READY', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);

    expect(h.latest().opcodes()).toEqual([OPCODE.IDENTIFY]);
    expect(h.latest().payload(0)).toEqual({
      token: 'access-token',
      client: { version: '0.1.0', os: 'windows' },
    });

    h.latest().deliver(ready(1, 'session-1'));
    expect(h.client.getStatus()).toBe('ready');
    expect(h.events).toHaveLength(1);
  });

  it('gives up before connecting when there is no access token', () => {
    const h = harness(null);
    h.client.start();
    expect(h.sockets).toHaveLength(0);
    expect(h.fatals).toEqual(['auth']);
  });
});

describe('reconnection', () => {
  it('resumes with the session and the last sequence after an unexpected close', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);
    h.latest().deliver(ready(1, 'session-1'));
    h.latest().deliver({ op: OPCODE.DISPATCH, s: 9, t: 'RESUMED', d: { replayed: 0 } });

    h.latest().serverClose(1006);
    expect(h.client.getStatus()).toBe('reconnecting');
    expect(h.sockets).toHaveLength(1);

    vi.advanceTimersByTime(1000);
    expect(h.sockets).toHaveLength(2);

    h.latest().deliver(HELLO);
    expect(h.latest().opcodes()).toEqual([OPCODE.RESUME]);
    expect(h.latest().payload(0)).toEqual({
      token: 'access-token',
      session_id: 'session-1',
      last_seq: 9,
    });
  });

  it('backs off further on each failed attempt', () => {
    const h = harness();
    h.client.start();
    h.latest().serverClose(1006);

    vi.advanceTimersByTime(999);
    expect(h.sockets).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(h.sockets).toHaveLength(2);

    h.latest().serverClose(1006);
    vi.advanceTimersByTime(1999);
    expect(h.sockets).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(h.sockets).toHaveLength(3);
  });

  it('reconnects at once, without backoff, when the server asks for it', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);
    h.latest().deliver(ready(1, 'session-1'));

    h.latest().deliver({ op: OPCODE.RECONNECT });
    expect(h.latest().closedWith).toBe(1000);

    vi.advanceTimersByTime(0);
    expect(h.sockets).toHaveLength(2);
  });

  it('identifies again, without a session, after INVALID_SESSION', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);
    h.latest().deliver(ready(1, 'session-1'));

    h.latest().deliver({ op: OPCODE.INVALID_SESSION, d: { resumable: false } });
    expect(h.latest().opcodes()).toEqual([OPCODE.IDENTIFY, OPCODE.IDENTIFY]);

    h.latest().serverClose(1006);
    vi.advanceTimersByTime(1000);
    h.latest().deliver(HELLO);
    expect(h.latest().opcodes()).toEqual([OPCODE.IDENTIFY]);
  });

  it('drops the session before reidentifying when the close says the frame was bad', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);
    h.latest().deliver(ready(1, 'session-1'));

    h.latest().serverClose(4002);
    vi.advanceTimersByTime(1000);
    h.latest().deliver(HELLO);
    expect(h.latest().opcodes()).toEqual([OPCODE.IDENTIFY]);
  });

  it('stops for good on an authentication failure', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);
    h.latest().serverClose(4001);

    vi.advanceTimersByTime(60_000);
    expect(h.sockets).toHaveLength(1);
    expect(h.fatals).toEqual(['auth']);
    expect(h.client.getStatus()).toBe('closed');
  });

  it('stops and asks for an update when the protocol version is too old', () => {
    const h = harness();
    h.client.start();
    h.latest().serverClose(4010);

    vi.advanceTimersByTime(60_000);
    expect(h.sockets).toHaveLength(1);
    expect(h.fatals).toEqual(['outdated_client']);
  });

  it('does not reconnect after stop', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);
    h.latest().deliver(ready(1, 'session-1'));
    h.client.stop();

    vi.advanceTimersByTime(60_000);
    expect(h.sockets).toHaveLength(1);
    expect(h.client.getStatus()).toBe('closed');
  });
});

/** 30 s plus the 5% jitter that `random: () => 0.5` produces. */
const BEAT_MS = 31_500;

describe('heartbeat', () => {
  it('beats on the interval and keeps beating while acked', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);
    h.latest().deliver(ready(1, 'session-1'));

    vi.advanceTimersByTime(BEAT_MS);
    expect(h.latest().opcodes()).toEqual([OPCODE.IDENTIFY, OPCODE.HEARTBEAT]);

    h.latest().deliver({ op: OPCODE.HEARTBEAT_ACK });
    vi.advanceTimersByTime(BEAT_MS);
    expect(h.latest().opcodes()).toEqual([OPCODE.IDENTIFY, OPCODE.HEARTBEAT, OPCODE.HEARTBEAT]);
    expect(h.latest().closedWith).toBeNull();
  });

  it('closes with 4900 and resumes when the acks stop coming', () => {
    const h = harness();
    h.client.start();
    h.latest().deliver(HELLO);
    h.latest().deliver(ready(1, 'session-1'));
    const first = h.latest();

    vi.advanceTimersByTime(BEAT_MS * 2);
    expect(first.closedWith).toBeNull();

    vi.advanceTimersByTime(BEAT_MS);
    expect(first.closedWith).toBe(CLOSE_ZOMBIE);

    vi.advanceTimersByTime(1000);
    expect(h.sockets).toHaveLength(2);
    h.latest().deliver(HELLO);
    expect(h.latest().opcodes()).toEqual([OPCODE.RESUME]);
  });
});
