import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RoomParticipant } from '../../api/types/RoomParticipant';
import { useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { RoomChrome } from './RoomChrome';

// O cromo importa o runtime só para trocar de preset, e o runtime abre canais do
// Tauri ao ser carregado — que não existem fora do aplicativo.
vi.mock('../../app/runtime', () => ({ media: { changePreset: vi.fn() } }));

/**
 * Issue #10: com alguém transmitindo, o corpo da sala dá lugar ao vídeo e a
 * lista de pessoas some — justamente quando se quer saber quem está do outro
 * lado da própria tela.
 */
function participant(id: string, username: string, publishing = false): RoomParticipant {
  return {
    user: { id, username, display_name: null, avatar_url: null },
    publishing,
  } as RoomParticipant;
}

function room(participants: RoomParticipant[]) {
  useRoomStore.setState({
    channelId: '1',
    guildId: '1',
    channelName: 'Geral',
    participantIds: participants.map((p) => p.user.id),
    participants: Object.fromEntries(participants.map((p) => [p.user.id, p])),
    publisherIds: participants.filter((p) => p.publishing).map((p) => p.user.id),
    lastLeaveReason: null,
  });
}

const noop = () => undefined;

function chrome() {
  return render(
    <RoomChrome onShare={noop} onStop={noop} onToggleFullscreen={noop} fullscreen={false} />,
  );
}

describe('quem está na sala (issue #10)', () => {
  beforeEach(() => {
    useMediaStore.getState().reset();
    room([participant('1', 'ana'), participant('2', 'bruno', true)]);
  });

  it('lista as pessoas mesmo com o vídeo ocupando a janela', () => {
    chrome();
    fireEvent.click(screen.getByRole('button', { name: /Quem está aqui \(2\)/ }));

    expect(screen.getByText('ana')).toBeDefined();
    expect(screen.getByText('bruno')).toBeDefined();
    expect(screen.getByText('transmitindo')).toBeDefined();
  });

  it('diz quem está vendo a sua tela, e conta uma pessoa uma vez só', () => {
    // Uma pessoa tem duas conexões quando também publica (ADR-0027); contá-la
    // duas vezes faria a lista mentir sobre quantos estão do outro lado.
    useMediaStore.getState().setPublishing(true, false);
    useMediaStore.getState().setViewerIds(['1', '1~pub']);
    chrome();
    fireEvent.click(screen.getByRole('button', { name: /Quem está aqui/ }));

    const watching = within(screen.getByRole('list', { name: 'Vendo a sua tela' }));
    expect(watching.getAllByRole('listitem')).toHaveLength(1);
    expect(watching.getByText('ana')).toBeDefined();
  });

  it('não promete saber de quem não abriu o aplicativo', () => {
    chrome();
    fireEvent.click(screen.getByRole('button', { name: /Quem está aqui/ }));
    expect(screen.getByText(/sem o ldktela aberto não aparece/)).toBeDefined();
  });
});
