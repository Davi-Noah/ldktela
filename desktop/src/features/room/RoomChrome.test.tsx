import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RoomParticipant } from '../../api/types/RoomParticipant';
import { useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { publicationId, type PublicationSource } from '../../media/publication';
import { RoomChrome } from './RoomChrome';

// O cromo importa o runtime só para trocar de preset, e o runtime abre canais do
// Tauri ao ser carregado — que não existem fora do aplicativo.
const setPublicationSubscribed = vi.fn();
const listCameras = vi.fn(() => Promise.resolve([]));
vi.mock('../../app/runtime', () => ({
  media: {
    changePreset: vi.fn(),
    setPublicationSubscribed: (...args: unknown[]) => {
      setPublicationSubscribed(...args);
    },
    listCameras: () => listCameras(),
    startCamera: vi.fn(),
    stopCamera: vi.fn(),
    switchCamera: vi.fn(),
  },
}));

/**
 * Issue #10: com alguém transmitindo, o corpo da sala dá lugar ao vídeo e a
 * lista de pessoas some — justamente quando se quer saber quem está do outro
 * lado da própria tela.
 */
function participant(id: string, username: string, ...live: PublicationSource[]): RoomParticipant {
  return {
    user: { id, username, display_name: null, avatar_url: null },
    publications: live.map((source) => ({ source, since: '2026-09-14T12:00:00Z' })),
  } as RoomParticipant;
}

function room(participants: RoomParticipant[]) {
  useRoomStore.setState({
    channelId: '1',
    guildId: '1',
    channelName: 'Geral',
    participantIds: participants.map((p) => p.user.id),
    participants: Object.fromEntries(participants.map((p) => [p.user.id, p])),
    publicationIds: participants.flatMap((p) =>
      p.publications.map((publication) => publicationId(p.user.id, publication.source)),
    ),
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
    room([participant('1', 'ana'), participant('2', 'bruno', 'screen')]);
  });

  it('lista as pessoas mesmo com o vídeo ocupando a janela', () => {
    chrome();
    fireEvent.click(screen.getByRole('button', { name: /Quem está aqui \(2\)/ }));

    expect(screen.getByText('ana')).toBeDefined();
    expect(screen.getByText('bruno')).toBeDefined();
    // Transmitindo sem a trilha ter chegado: não há do que sair ainda, então a
    // linha informa em vez de oferecer um botão que não faria nada.
    expect(screen.getByText('no ar')).toBeDefined();
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
    setPublicationSubscribed.mockClear();
    room([participant('1', 'ana', 'screen'), participant('2', 'bruno', 'screen')]);
  });

  it('está na lista de pessoas, que é o único lugar que mostra a sala inteira', () => {
    // O ladrilho de quem se deixou de assistir não existe mais (ADR-0036): sem
    // este botão, a tela sumiria sem caminho nenhum de volta.
    useMediaStore.getState().addTrack('1:screen', 'video');
    useMediaStore.getState().setSubscribed('1:screen', false);
    chrome();
    fireEvent.click(screen.getByRole('button', { name: /Quem está aqui/ }));

    fireEvent.click(screen.getByRole('button', { name: 'Entrar' }));
    expect(setPublicationSubscribed).toHaveBeenCalledWith('1:screen', true);
  });

  it('conta no cabeçalho quantas telas ficaram de fora', () => {
    useMediaStore.getState().addTrack('1:screen', 'video');
    useMediaStore.getState().setSubscribed('1:screen', false);
    chrome();
    expect(screen.getByText(/1 fora/)).toBeDefined();
  });
});

describe('foco exclusivo (issue #7)', () => {
  beforeEach(() => {
    useMediaStore.getState().reset();
    room([participant('1', 'ana', 'screen'), participant('2', 'bruno', 'screen')]);
  });

  it('não é oferecido com uma tela só, porque não há o que esconder', () => {
    useMediaStore.getState().addTrack('1:screen', 'video');
    useMediaStore.getState().focus('1:screen');
    chrome();
    expect(screen.queryByRole('button', { name: /só esta tela/ })).toBeNull();
  });

  it('aparece assim que existe uma segunda tela', () => {
    useMediaStore.getState().addTrack('1:screen', 'video');
    useMediaStore.getState().addTrack('2:screen', 'video');
    useMediaStore.getState().focus('1:screen');
    chrome();
    expect(screen.getByRole('button', { name: /só esta tela/ })).toBeDefined();
  });
});
