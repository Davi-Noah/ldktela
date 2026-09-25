import {
  OPCODE,
  encodeHeartbeat,
  encodeIdentify,
  encodeResume,
  parseServerFrame,
} from './protocol';

function frame(value: unknown): string {
  return JSON.stringify(value);
}

describe('parseServerFrame', () => {
  it('reads HELLO with its heartbeat interval', () => {
    const parsed = parseServerFrame(
      frame({ op: 1, d: { heartbeat_interval_ms: 30000, session_ttl_ms: 90000 } }),
    );
    expect(parsed).toEqual({
      kind: 'hello',
      hello: { heartbeat_interval_ms: 30000, session_ttl_ms: 90000 },
    });
  });

  it('rejects a HELLO without a usable interval', () => {
    expect(parseServerFrame(frame({ op: 1, d: { heartbeat_interval_ms: 0 } }))).toBeNull();
    expect(parseServerFrame(frame({ op: 1 }))).toBeNull();
  });

  it('reads a dispatch into the generated event shape', () => {
    const parsed = parseServerFrame(
      frame({
        op: 0,
        s: 7,
        t: 'ROOM_LEAVE',
        d: { discord_channel_id: '42', reason: 'access_revoked' },
      }),
    );
    expect(parsed).toEqual({
      kind: 'dispatch',
      seq: 7,
      event: { t: 'ROOM_LEAVE', d: { discord_channel_id: '42', reason: 'access_revoked' } },
    });
  });

  it('reads the private call end dispatch', () => {
    const parsed = parseServerFrame(
      frame({ op: 0, s: 8, t: 'PRIVATE_CALL_END', d: { call_id: 'call-1' } }),
    );
    expect(parsed).toEqual({
      kind: 'dispatch',
      seq: 8,
      event: { t: 'PRIVATE_CALL_END', d: { call_id: 'call-1' } },
    });
  });

  it('reads the private call join dispatch', () => {
    const call = {
      id: 'call-1',
      owner: { id: 'owner-1', username: 'owner', display_name: 'Owner' },
      guest: { id: 'guest-1', username: 'guest', display_name: 'Guest' },
    };
    const parsed = parseServerFrame(frame({ op: 0, s: 9, t: 'PRIVATE_CALL_JOIN', d: { call } }));
    expect(parsed).toEqual({
      kind: 'dispatch',
      seq: 9,
      event: { t: 'PRIVATE_CALL_JOIN', d: { call } },
    });
  });

  it('drops a dispatch whose event name it does not know', () => {
    expect(parseServerFrame(frame({ op: 0, s: 1, t: 'MESSAGE_CREATE', d: {} }))).toBeNull();
  });

  it('drops a dispatch without a sequence or payload', () => {
    expect(parseServerFrame(frame({ op: 0, t: 'RESUMED', d: { replayed: 1 } }))).toBeNull();
    expect(parseServerFrame(frame({ op: 0, s: 1, t: 'RESUMED' }))).toBeNull();
  });

  it('reads the control frames the client acts on', () => {
    expect(parseServerFrame(frame({ op: 5 }))).toEqual({ kind: 'heartbeat_ack' });
    expect(parseServerFrame(frame({ op: 6, d: { resumable: false } }))).toEqual({
      kind: 'invalid_session',
      resumable: false,
    });
    expect(parseServerFrame(frame({ op: 7 }))).toEqual({ kind: 'reconnect' });
  });

  it('drops anything that is not a frame', () => {
    expect(parseServerFrame('not json')).toBeNull();
    expect(parseServerFrame(frame({ op: 'hello' }))).toBeNull();
    expect(parseServerFrame(frame([1, 2, 3]))).toBeNull();
    expect(parseServerFrame(new ArrayBuffer(4))).toBeNull();
  });
});

describe('client frames', () => {
  it('sends IDENTIFY with the access token and client info', () => {
    const encoded: unknown = JSON.parse(
      encodeIdentify('token-a', { version: '0.1.0', os: 'windows' }),
    );
    expect(encoded).toEqual({
      op: OPCODE.IDENTIFY,
      d: { token: 'token-a', client: { version: '0.1.0', os: 'windows' } },
    });
  });

  it('sends RESUME with the session and the last sequence seen', () => {
    const encoded: unknown = JSON.parse(encodeResume('token-a', 'session-1', 12));
    expect(encoded).toEqual({
      op: OPCODE.RESUME,
      d: { token: 'token-a', session_id: 'session-1', last_seq: 12 },
    });
  });

  it('sends a bare heartbeat', () => {
    expect(JSON.parse(encodeHeartbeat())).toEqual({ op: OPCODE.HEARTBEAT });
  });
});
