import type { DispatchEvent } from '../api/types/DispatchEvent';
import type { RoomParticipant } from '../api/types/RoomParticipant';
import type { RoomState } from '../api/types/RoomState';
import { EMPTY_ROOM, applyRoomEvent, roomFromState } from './room';

function participant(id: string, publishing = false): RoomParticipant {
  return {
    user: {
      id,
      discord_user_id: `d-${id}`,
      username: id,
      display_name: null,
      avatar_url: null,
    },
    publishing,
  };
}

const ROOM: RoomState = {
  discord_channel_id: '100',
  discord_guild_id: '9',
  channel_name: 'jogos',
  participants: [participant('ana'), participant('bia', true)],
};

const JOIN: DispatchEvent = { t: 'ROOM_JOIN', d: ROOM };

describe('roomFromState', () => {
  it('normalises participants by id and keeps the server order', () => {
    const room = roomFromState(ROOM);
    expect(room.channelId).toBe('100');
    expect(room.channelName).toBe('jogos');
    expect(room.participantIds).toEqual(['ana', 'bia']);
    expect(room.participants.bia?.publishing).toBe(true);
    expect(room.publisherIds).toEqual(['bia']);
  });
});

describe('applyRoomEvent', () => {
  it('replaces the room on ROOM_JOIN', () => {
    expect(applyRoomEvent(EMPTY_ROOM, JOIN).channelId).toBe('100');
  });

  it('clears the room on READY without one', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const readyWithoutRoom: DispatchEvent = {
      t: 'READY',
      d: {
        session_id: 's',
        user: {
          id: 'ana',
          discord_user_id: 'd',
          username: 'ana',
          display_name: null,
          avatar_url: null,
          created_at: '2026-01-01T00:00:00Z',
        },
        heartbeat_interval_ms: 30_000,
      },
    };
    expect(applyRoomEvent(room, readyWithoutRoom).channelId).toBeNull();
  });

  it('adds a participant once, however many times the event arrives', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const add: DispatchEvent = {
      t: 'ROOM_PARTICIPANT_ADD',
      d: { discord_channel_id: '100', participant: participant('caio') },
    };
    const once = applyRoomEvent(room, add);
    expect(once.participantIds).toEqual(['ana', 'bia', 'caio']);

    const twice = applyRoomEvent(once, add);
    expect(twice).toBe(once);
  });

  it('removes a participant from every collection', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const remove: DispatchEvent = {
      t: 'ROOM_PARTICIPANT_REMOVE',
      d: { discord_channel_id: '100', user_id: 'bia' },
    };
    const after = applyRoomEvent(room, remove);
    expect(after.participantIds).toEqual(['ana']);
    expect(after.participants.bia).toBeUndefined();
    expect(after.publisherIds).toEqual([]);
    expect(applyRoomEvent(after, remove)).toBe(after);
  });

  it('tracks who is publishing across SHARE_START and SHARE_STOP', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const start: DispatchEvent = {
      t: 'SHARE_START',
      d: { discord_channel_id: '100', user_id: 'ana' },
    };
    const started = applyRoomEvent(room, start);
    expect(started.publisherIds).toEqual(['bia', 'ana']);
    expect(started.participants.ana?.publishing).toBe(true);
    expect(applyRoomEvent(started, start)).toBe(started);

    const stopped = applyRoomEvent(started, {
      t: 'SHARE_STOP',
      d: { discord_channel_id: '100', user_id: 'ana' },
    });
    expect(stopped.publisherIds).toEqual(['bia']);
    expect(stopped.participants.ana?.publishing).toBe(false);
  });

  it('ignores events aimed at a channel we are not in', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const events: DispatchEvent[] = [
      {
        t: 'ROOM_PARTICIPANT_ADD',
        d: { discord_channel_id: '999', participant: participant('x') },
      },
      { t: 'ROOM_PARTICIPANT_REMOVE', d: { discord_channel_id: '999', user_id: 'ana' } },
      { t: 'SHARE_START', d: { discord_channel_id: '999', user_id: 'ana' } },
      { t: 'ROOM_LEAVE', d: { discord_channel_id: '999', reason: 'left' } },
    ];
    for (const event of events) {
      expect(applyRoomEvent(room, event)).toBe(room);
    }
  });

  it('drops a share event for a user it has never seen', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const event: DispatchEvent = {
      t: 'SHARE_START',
      d: { discord_channel_id: '100', user_id: 'fantasma' },
    };
    expect(applyRoomEvent(room, event)).toBe(room);
  });

  it('keeps the reason when the server takes us out of the room', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const left = applyRoomEvent(room, {
      t: 'ROOM_LEAVE',
      d: { discord_channel_id: '100', reason: 'access_revoked' },
    });
    expect(left.channelId).toBeNull();
    expect(left.participantIds).toEqual([]);
    expect(left.lastLeaveReason).toBe('access_revoked');
  });

  it('leaves the room untouched on RESUMED', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    expect(applyRoomEvent(room, { t: 'RESUMED', d: { replayed: 3 } })).toBe(room);
  });
});
