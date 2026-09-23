import { create } from 'zustand';
import type { DispatchEvent } from '../api/types/DispatchEvent';
import type { Publication } from '../api/types/Publication';
import type { RoomLeaveReason } from '../api/types/RoomLeaveReason';
import type { RoomParticipant } from '../api/types/RoomParticipant';
import type { RoomState } from '../api/types/RoomState';
import type { Snowflake } from '../api/types/Snowflake';
import type { Timestamp } from '../api/types/Timestamp';
import {
  publicationId,
  SOURCE_ORDER,
  type PublicationId,
  type PublicationSource,
} from '../media/publication';

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
  /**
   * Quem está no ar, por publicação e não por pessoa (ADR-0038).
   *
   * Ordem de chegada entre pessoas, e tela antes de câmera dentro de cada uma:
   * ligar a câmera acrescenta um ladrilho ao fim, e nunca remexe os que já
   * estavam na grade.
   */
  publicationIds: PublicationId[];
  lastLeaveReason: RoomLeaveReason | null;
}

export const EMPTY_ROOM: RoomSnapshot = {
  channelId: null,
  guildId: null,
  channelName: null,
  participantIds: [],
  participants: {},
  publicationIds: [],
  lastLeaveReason: null,
};

/** As publicações de uma pessoa, em ordem fixa de fonte. */
export function publicationsOf(participant: RoomParticipant | undefined): Publication[] {
  if (participant === undefined) {
    return [];
  }
  return [...participant.publications].sort(
    (a, b) => SOURCE_ORDER.indexOf(a.source) - SOURCE_ORDER.indexOf(b.source),
  );
}

/** Quando esta publicação entrou no ar, ou `null` se ela não está. */
export function publicationSince(
  participant: RoomParticipant | undefined,
  source: PublicationSource,
): Timestamp | null {
  return participant?.publications.find((p) => p.source === source)?.since ?? null;
}

export function roomFromState(state: RoomState): RoomSnapshot {
  const participants: Record<string, RoomParticipant> = {};
  const participantIds: string[] = [];
  const publicationIds: PublicationId[] = [];
  for (const participant of state.participants) {
    const id = participant.user.id;
    if (participants[id] !== undefined) {
      continue;
    }
    participants[id] = participant;
    participantIds.push(id);
    for (const publication of publicationsOf(participant)) {
      publicationIds.push(publicationId(id, publication.source));
    }
  }
  return {
    channelId: state.discord_channel_id,
    guildId: state.discord_guild_id,
    channelName: state.channel_name,
    participantIds,
    participants,
    publicationIds,
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
      return setPublishing(
        room,
        event.d.discord_channel_id,
        event.d.user_id,
        event.d.source,
        event.d.started_at,
      );
    case 'SHARE_STOP':
      return setPublishing(room, event.d.discord_channel_id, event.d.user_id, event.d.source, null);
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
  if (known !== undefined && samePublications(known, participant)) {
    return room;
  }
  const participants = { ...room.participants, [id]: participant };
  const participantIds = known === undefined ? [...room.participantIds, id] : room.participantIds;

  let publicationIds = room.publicationIds;
  for (const source of SOURCE_ORDER) {
    const live = participant.publications.some((p) => p.source === source);
    publicationIds = withPublication(publicationIds, publicationId(id, source), live);
  }
  return { ...room, participants, participantIds, publicationIds };
}

function samePublications(known: RoomParticipant, fresh: RoomParticipant): boolean {
  if (known.publications.length !== fresh.publications.length) {
    return false;
  }
  return known.publications.every((publication) =>
    fresh.publications.some((p) => p.source === publication.source && p.since === publication.since),
  );
}

function removeParticipant(room: RoomSnapshot, channelId: Snowflake, userId: string): RoomSnapshot {
  if (room.channelId !== channelId || room.participants[userId] === undefined) {
    return room;
  }
  const participants = { ...room.participants };
  delete participants[userId];
  let publicationIds = room.publicationIds;
  for (const source of SOURCE_ORDER) {
    publicationIds = withPublication(publicationIds, publicationId(userId, source), false);
  }
  return {
    ...room,
    participants,
    participantIds: room.participantIds.filter((id) => id !== userId),
    publicationIds,
  };
}

/**
 * `since` is the server's start time on SHARE_START and `null` on SHARE_STOP.
 *
 * It has to come from the server: a viewer who joins twenty minutes in must see
 * twenty minutes, not zero (RF-34).
 *
 * Só a fonte do evento muda: parar a câmera não pode encostar no relógio da
 * tela, que continua no ar (ADR-0038).
 */
function setPublishing(
  room: RoomSnapshot,
  channelId: Snowflake,
  userId: string,
  source: PublicationSource,
  since: Timestamp | null,
): RoomSnapshot {
  const participant = room.participants[userId];
  // A share event for someone we do not know yet is dropped: the matching
  // ROOM_PARTICIPANT_ADD carries the same publications.
  if (room.channelId !== channelId || participant === undefined) {
    return room;
  }

  const current = participant.publications.find((p) => p.source === source);
  if (since === null ? current === undefined : current?.since === since) {
    return room;
  }

  const publications =
    since === null
      ? participant.publications.filter((p) => p.source !== source)
      : [...participant.publications.filter((p) => p.source !== source), { source, since }];

  return {
    ...room,
    participants: { ...room.participants, [userId]: { ...participant, publications } },
    publicationIds: withPublication(
      room.publicationIds,
      publicationId(userId, source),
      since !== null,
    ),
  };
}

/**
 * Acrescenta ao fim e remove no lugar.
 *
 * Quem já estava na grade não pode mudar de posição porque outra pessoa ligou a
 * câmera: o ladrilho que a pessoa estava olhando saltaria para o lado.
 */
function withPublication(
  current: PublicationId[],
  id: PublicationId,
  live: boolean,
): PublicationId[] {
  const present = current.includes(id);
  if (live === present) {
    return current;
  }
  return live ? [...current, id] : current.filter((existing) => existing !== id);
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
