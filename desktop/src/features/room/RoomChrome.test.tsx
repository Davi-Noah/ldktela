import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RoomParticipant } from '../../api/types/RoomParticipant';
import { useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { RoomChrome } from './RoomChrome';

// O cromo importa o runtime só para trocar de preset, e o runtime abre canais do
// Tauri ao ser carregado — que não existem fora do aplicativo.
const setScreenSubscribed = vi.fn();
vi.mock('../../app/runtime', () => ({
  media: {
    changePreset: vi.fn(),
    setScreenSubscribed: (...args: unknown[]) => {
      setScreenSubscribed(...args);
    },
  },
}));

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

describe('o caminho de volta para uma tela que se deixou de assistir (issue #7)', () => {
  beforeEach(() => {
    useMediaStore.getState().reset();
    setScreenSubscribed.mockClear();
    room([participant('1', 'ana', true), participant('2', 'bruno', true)]);
  });

  it('está na lista de pessoas, que é o único lugar que mostra a sala inteira', () => {
    // O ladrilho de quem se deixou de assistir não existe mais (ADR-0036): sem
    // este botão, a tela sumiria sem caminho nenhum de volta.
    useMediaStore.getState().addScreen('1', 'video');
    useMediaStore.getState().setScreenSubscribed('1', false);
    chrome();
    fireEvent.click(screen.getByRole('button', { name: /Quem está aqui/ }));

    fireEvent.click(screen.getByRole('button', { name: 'Ver a tela de ana' }));
    expect(setScreenSubscribed).toHaveBeenCalledWith('1', true);
  });

  it('conta no cabeçalho quantas telas ficaram de fora', () => {
    useMediaStore.getState().addScreen('1', 'video');
    useMediaStore.getState().setScreenSubscribed('1', false);
    chrome();
    expect(screen.getByText(/1 fora/)).toBeDefined();
  });
});

describe('foco exclusivo (issue #7)', () => {
  beforeEach(() => {
    useMediaStore.getState().reset();
    room([participant('1', 'ana', true), participant('2', 'bruno', true)]);
  });

  it('não é oferecido com uma tela só, porque não há o que esconder', () => {
    useMediaStore.getState().addScreen('1', 'video');
    useMediaStore.getState().focus('1');
    chrome();
    expect(screen.queryByRole('button', { name: /só esta tela/ })).toBeNull();
  });

  it('aparece assim que existe uma segunda tela', () => {
    useMediaStore.getState().addScreen('1', 'video');
    useMediaStore.getState().addScreen('2', 'video');
    useMediaStore.getState().focus('1');
    chrome();
    expect(screen.getByRole('button', { name: /só esta tela/ })).toBeDefined();
  });
});
