import type { DispatchEvent } from '../api/types/DispatchEvent';
import type { RoomParticipant } from '../api/types/RoomParticipant';
import type { RoomState } from '../api/types/RoomState';
import type { PublicationSource } from '../media/publication';
import { EMPTY_ROOM, applyRoomEvent, publicationSince, roomFromState } from './room';

const SINCE = '2026-09-14T12:00:00Z';

function participant(id: string, ...live: PublicationSource[]): RoomParticipant {
  return {
    user: {
      id,
      discord_user_id: `d-${id}`,
      username: id,
      display_name: null,
      avatar_url: null,
    },
    publications: live.map((source) => ({ source, since: SINCE })),
  };
}

const ROOM: RoomState = {
  discord_channel_id: '100',
  discord_guild_id: '9',
  channel_name: 'jogos',
  participants: [participant('ana'), participant('bia', 'screen')],
};

const JOIN: DispatchEvent = { t: 'ROOM_JOIN', d: ROOM };

describe('roomFromState', () => {
  it('normalises participants by id and keeps the server order', () => {
    const room = roomFromState(ROOM);
    expect(room.channelId).toBe('100');
    expect(room.channelName).toBe('jogos');
    expect(room.participantIds).toEqual(['ana', 'bia']);
    expect(room.participants.bia?.publications).toHaveLength(1);
    expect(room.publicationIds).toEqual(['bia:screen']);
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
    expect(after.publicationIds).toEqual([]);
    expect(applyRoomEvent(after, remove)).toBe(after);
  });

  it('tracks who is publishing across SHARE_START and SHARE_STOP', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const start: DispatchEvent = {
      t: 'SHARE_START',
      d: { discord_channel_id: '100', user_id: 'ana', source: 'screen', started_at: SINCE },
    };
    const started = applyRoomEvent(room, start);
    expect(started.publicationIds).toEqual(['bia:screen', 'ana:screen']);
    expect(publicationSince(started.participants.ana, 'screen')).toBe(SINCE);
    expect(applyRoomEvent(started, start)).toBe(started);

    const stopped = applyRoomEvent(started, {
      t: 'SHARE_STOP',
      d: { discord_channel_id: '100', user_id: 'ana', source: 'screen' },
    });
    expect(stopped.publicationIds).toEqual(['bia:screen']);
    expect(stopped.participants.ana?.publications).toEqual([]);
  });

  it('ignores events aimed at a channel we are not in', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const events: DispatchEvent[] = [
      {
        t: 'ROOM_PARTICIPANT_ADD',
        d: { discord_channel_id: '999', participant: participant('x') },
      },
      { t: 'ROOM_PARTICIPANT_REMOVE', d: { discord_channel_id: '999', user_id: 'ana' } },
      {
        t: 'SHARE_START',
        d: { discord_channel_id: '999', user_id: 'ana', source: 'screen', started_at: SINCE },
      },
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
      d: { discord_channel_id: '100', user_id: 'fantasma', source: 'screen', started_at: SINCE },
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

/**
 * A câmera é uma segunda publicação da mesma pessoa (ADR-0038). Todo teste aqui
 * existe porque, com a pessoa como unidade, uma fonte apagava a outra.
 */
describe('duas fontes por pessoa (ADR-0038)', () => {
  it('conta uma publicação por fonte, tela antes de câmera', () => {
    const room = roomFromState({
      ...ROOM,
      participants: [participant('ana', 'camera', 'screen')],
    });
    expect(room.publicationIds).toEqual(['ana:screen', 'ana:camera']);
  });

  it('ligar a câmera não remexe quem já estava na grade', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const withCamera = applyRoomEvent(room, {
      t: 'SHARE_START',
      d: { discord_channel_id: '100', user_id: 'bia', source: 'camera', started_at: SINCE },
    });
    expect(withCamera.publicationIds).toEqual(['bia:screen', 'bia:camera']);
  });

  it('parar a câmera deixa a tela da mesma pessoa no ar', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const both = applyRoomEvent(room, {
      t: 'SHARE_START',
      d: { discord_channel_id: '100', user_id: 'bia', source: 'camera', started_at: SINCE },
    });
    const onlyScreen = applyRoomEvent(both, {
      t: 'SHARE_STOP',
      d: { discord_channel_id: '100', user_id: 'bia', source: 'camera' },
    });

    expect(onlyScreen.publicationIds).toEqual(['bia:screen']);
    expect(publicationSince(onlyScreen.participants.bia, 'screen')).toBe(SINCE);
    expect(publicationSince(onlyScreen.participants.bia, 'camera')).toBeNull();
  });

  it('cada fonte tem o seu relógio', () => {
    const later = '2026-09-14T12:30:00Z';
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const both = applyRoomEvent(room, {
      t: 'SHARE_START',
      d: { discord_channel_id: '100', user_id: 'bia', source: 'camera', started_at: later },
    });
    expect(publicationSince(both.participants.bia, 'screen')).toBe(SINCE);
    expect(publicationSince(both.participants.bia, 'camera')).toBe(later);
  });

  it('sair da sala leva as duas publicações junto', () => {
    const room = applyRoomEvent(EMPTY_ROOM, JOIN);
    const both = applyRoomEvent(room, {
      t: 'SHARE_START',
      d: { discord_channel_id: '100', user_id: 'bia', source: 'camera', started_at: SINCE },
    });
    const gone = applyRoomEvent(both, {
      t: 'ROOM_PARTICIPANT_REMOVE',
      d: { discord_channel_id: '100', user_id: 'bia' },
    });
    expect(gone.publicationIds).toEqual([]);
  });
});
