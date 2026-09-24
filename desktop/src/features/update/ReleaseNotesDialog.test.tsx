import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useReleaseNotesStore } from '../../store/releaseNotes';
import { readNotes } from './notes';
import { ReleaseNotesDialog } from './ReleaseNotesDialog';

// O registro de "já vista" mora no core, que não existe fora do aplicativo.
const invoke = vi.fn<(command: string, args?: unknown) => Promise<void>>(() => Promise.resolve());
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));

const NOTES = readNotes(
  [
    '# ldktela v2.0.1 — Correções',
    '',
    '## Corrigido',
    '',
    '- **O som** de "tela começou" não entra mais em loop.',
  ].join('\n'),
);

describe('o modal de novidades', () => {
  beforeEach(() => {
    invoke.mockClear();
    useReleaseNotesStore.getState().clear();
  });

  it('não existe enquanto não há novidade', () => {
    render(<ReleaseNotesDialog />);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('mostra o texto do release, com o título dele no topo', () => {
    render(<ReleaseNotesDialog />);
    act(() => {
      useReleaseNotesStore.getState().show('2.0.1', NOTES);
    });

    const dialog = screen.getByRole('dialog', { name: 'ldktela v2.0.1 — Correções' });
    expect(dialog.textContent).toContain('Corrigido');
    expect(screen.getByText('O som').tagName).toBe('STRONG');
  });

  /**
   * O aplicativo abre na bandeja, e o modal pode estar montado numa janela que
   * ninguém abriu. Só fechar prova que alguém viu (ADR-0040, decisão 4).
   */
  it('só marca a versão como vista quando é fechado', () => {
    render(<ReleaseNotesDialog />);
    act(() => {
      useReleaseNotesStore.getState().show('2.0.1', NOTES);
    });
    expect(invoke).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Entendi' }));

    expect(invoke).toHaveBeenCalledWith('release_notes_mark_seen', { version: '2.0.1' });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('fechar pelo Escape também conta como visto', () => {
    render(<ReleaseNotesDialog />);
    act(() => {
      useReleaseNotesStore.getState().show('2.0.1', NOTES);
    });

    fireEvent.keyDown(document, { key: 'Escape' });

    expect(invoke).toHaveBeenCalledWith('release_notes_mark_seen', { version: '2.0.1' });
  });
});
