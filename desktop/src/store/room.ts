import { create } from 'zustand';
import type { DispatchEvent } from '../api/types/DispatchEvent';
import type { RoomLeaveReason } from '../api/types/RoomLeaveReason';
import type { RoomParticipant } from '../api/types/RoomParticipant';
import type { RoomState } from '../api/types/RoomState';
import type { Snowflake } from '../api/types/Snowflake';
import type { Timestamp } from '../api/types/Timestamp';

/**
 * The room the user is in, normalised by user id. There is no room picker: this
 * is whatever Discord voice channel the server says the user joined (ADR-0011).
 */
export interface RoomSnapshot {
  channelId: Snowflake | null;
  guildId: Snowflake | null;
  channelName: string | null;
  /** Insertion order, so the list does not jump around between renders. */
  participantIds: string[];
  participants: Record<string, RoomParticipant>;
  publisherIds: string[];
  lastLeaveReason: RoomLeaveReason | null;
}

export const EMPTY_ROOM: RoomSnapshot = {
  channelId: null,
  guildId: null,
  channelName: null,
  participantIds: [],
  participants: {},
  publisherIds: [],
  lastLeaveReason: null,
};

export function roomFromState(state: RoomState): RoomSnapshot {
  const participants: Record<string, RoomParticipant> = {};
  const participantIds: string[] = [];
  const publisherIds: string[] = [];
  for (const participant of state.participants) {
    const id = participant.user.id;
    if (participants[id] !== undefined) {
      continue;
    }
    participants[id] = participant;
    participantIds.push(id);
    if (participant.publishing) {
      publisherIds.push(id);
    }
  }
  return {
    channelId: state.discord_channel_id,
    guildId: state.discord_guild_id,
    channelName: state.channel_name,
    participantIds,
    participants,
    publisherIds,
    lastLeaveReason: null,
  };
}

/**
 * Every branch returns the input untouched when nothing changed: after a resume
 * the same event can arrive twice, and a new object would re-render the tree for
 * no reason (websocket.md §6.3).
 */
export function applyRoomEvent(room: RoomSnapshot, event: DispatchEvent): RoomSnapshot {
  switch (event.t) {
    case 'READY':
      return event.d.room === undefined ? EMPTY_ROOM : roomFromState(event.d.room);
    case 'ROOM_JOIN':
      return roomFromState(event.d);
    case 'ROOM_LEAVE':
      if (room.channelId !== event.d.discord_channel_id) {
        return room;
      }
      return { ...EMPTY_ROOM, lastLeaveReason: event.d.reason };
    case 'ROOM_PARTICIPANT_ADD':
      return addParticipant(room, event.d.discord_channel_id, event.d.participant);
    case 'ROOM_PARTICIPANT_REMOVE':
      return removeParticipant(room, event.d.discord_channel_id, event.d.user_id);
    case 'SHARE_START':
      return setPublishing(room, event.d.discord_channel_id, event.d.user_id, event.d.started_at);
    case 'SHARE_STOP':
      return setPublishing(room, event.d.discord_channel_id, event.d.user_id, null);
    case 'RESUMED':
      return room;
  }
}

function addParticipant(
  room: RoomSnapshot,
  channelId: Snowflake,
  participant: RoomParticipant,
): RoomSnapshot {
  if (room.channelId !== channelId) {
    return room;
  }
  const id = participant.user.id;
  const known = room.participants[id];
  if (
    known !== undefined &&
    known.publishing === participant.publishing &&
    known.publishing_since === participant.publishing_since
  ) {
    return room;
  }
  const participants = { ...room.participants, [id]: participant };
  const participantIds = known === undefined ? [...room.participantIds, id] : room.participantIds;
  const publisherIds = withPublisher(room.publisherIds, id, participant.publishing);
  return { ...room, participants, participantIds, publisherIds };
}

function removeParticipant(room: RoomSnapshot, channelId: Snowflake, userId: string): RoomSnapshot {
  if (room.channelId !== channelId || room.participants[userId] === undefined) {
    return room;
  }
  const participants = { ...room.participants };
  delete participants[userId];
  return {
    ...room,
    participants,
    participantIds: room.participantIds.filter((id) => id !== userId),
    publisherIds: withPublisher(room.publisherIds, userId, false),
  };
}

/**
 * `since` is the server's start time on SHARE_START and `null` on SHARE_STOP.
 *
 * It has to come from the server: a viewer who joins twenty minutes in must see
 * twenty minutes, not zero (RF-34).
 */
function setPublishing(
  room: RoomSnapshot,
  channelId: Snowflake,
  userId: string,
  since: Timestamp | null,
): RoomSnapshot {
  const participant = room.participants[userId];
  // A share event for someone we do not know yet is dropped: the matching
  // ROOM_PARTICIPANT_ADD carries the same `publishing` flag.
  if (room.channelId !== channelId || participant === undefined) {
    return room;
  }
  const publishing = since !== null;
  if (
    participant.publishing === publishing &&
    participant.publishing_since === (since ?? undefined)
  ) {
    return room;
  }
  const updated: RoomParticipant = {
    ...participant,
    publishing,
    publishing_since: since ?? undefined,
  };
  return {
    ...room,
    participants: { ...room.participants, [userId]: updated },
    publisherIds: withPublisher(room.publisherIds, userId, publishing),
  };
}

function withPublisher(current: string[], userId: string, publishing: boolean): string[] {
  const present = current.includes(userId);
  if (publishing === present) {
    return current;
  }
  return publishing ? [...current, userId] : current.filter((id) => id !== userId);
}

interface RoomStore extends RoomSnapshot {
  apply: (event: DispatchEvent) => void;
  reset: () => void;
}

export const useRoomStore = create<RoomStore>()((set) => ({
  ...EMPTY_ROOM,
  apply: (event) => {
    set((state) => applyRoomEvent(state, event));
  },
  reset: () => {
    set(EMPTY_ROOM);
  },
}));
