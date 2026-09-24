import { describe, expect, it } from 'vitest';
import { compareVersions, notesVerdict, readInlines, readNotes } from './notes';

describe('quando as novidades aparecem', () => {
  it('aparecem depois de uma atualização', () => {
    expect(notesVerdict({ seen: '2.0.0', current: '2.0.1', hadSession: true })).toBe('show');
  });

  it('não aparecem de novo na mesma versão', () => {
    expect(notesVerdict({ seen: '2.0.1', current: '2.0.1', hadSession: true })).toBe('nothing');
  });

  it('não aparecem numa instalação nova, que só registra a versão', () => {
    expect(notesVerdict({ seen: null, current: '2.0.1', hadSession: false })).toBe('record');
  });

  /**
   * A versão que traz este recurso não encontra registro nenhum, mesmo para
   * quem já usava o aplicativo. O cofre com sessão é o que diz que foi
   * atualização (ADR-0040, decisão 3).
   */
  it('aparecem para quem já usava uma versão sem registro', () => {
    expect(notesVerdict({ seen: null, current: '2.0.1', hadSession: true })).toBe('show');
  });

  it('não aparecem ao voltar para uma versão mais velha', () => {
    expect(notesVerdict({ seen: '2.1.0', current: '2.0.1', hadSession: true })).toBe('record');
  });

  it('compara número a número, e não como texto', () => {
    // Como texto, "2.10.0" viria antes de "2.9.0".
    expect(compareVersions('2.9.0', '2.10.0')).toBe(-1);
    expect(compareVersions('10.0.0', '9.9.9')).toBe(1);
    expect(compareVersions('2.0.1', '2.0.1')).toBe(0);
  });

  it('não adivinha com uma versão que não se lê', () => {
    expect(compareVersions('lixo', '2.0.1')).toBeNull();
    expect(notesVerdict({ seen: 'lixo', current: '2.0.1', hadSession: true })).toBe('record');
  });
});

describe('a leitura do texto do release', () => {
  it('usa o título do release como título do modal, e não o repete', () => {
    const notes = readNotes('# ldktela v2.0.1 — Correções\n\nUma atualização curta.');
    expect(notes.title).toBe('ldktela v2.0.1 — Correções');
    expect(notes.blocks).toEqual([
      { kind: 'paragraph', inlines: [{ kind: 'text', text: 'Uma atualização curta.' }] },
    ]);
  });

  it('lê seções, listas e parágrafos quebrados em várias linhas', () => {
    const notes = readNotes(
      [
        '## Corrigido',
        '',
        '- **O som** não entra mais em loop.',
        '- Uma câmera que falha',
        '  não trava mais.',
        '',
        'Primeira linha',
        'e a segunda.',
      ].join('\r\n'),
    );
    expect(notes.title).toBeNull();
    expect(notes.blocks).toEqual([
      { kind: 'heading', level: 2, inlines: [{ kind: 'text', text: 'Corrigido' }] },
      {
        kind: 'list',
        ordered: false,
        items: [
          [
            { kind: 'strong', text: 'O som' },
            { kind: 'text', text: ' não entra mais em loop.' },
          ],
          [{ kind: 'text', text: 'Uma câmera que falha não trava mais.' }],
        ],
      },
      { kind: 'paragraph', inlines: [{ kind: 'text', text: 'Primeira linha e a segunda.' }] },
    ]);
  });

  it('protege o que está dentro de código', () => {
    expect(readInlines('use `Ctrl+Shift+E` para **parar**')).toEqual([
      { kind: 'text', text: 'use ' },
      { kind: 'code', text: 'Ctrl+Shift+E' },
      { kind: 'text', text: ' para ' },
      { kind: 'strong', text: 'parar' },
    ]);
  });

  /** Nota técnica cita identificadores com sublinhado; itálico com `_` os morderia. */
  it('não trata sublinhado como itálico', () => {
    expect(readInlines('a trilha screen_share_audio')).toEqual([
      { kind: 'text', text: 'a trilha screen_share_audio' },
    ]);
  });

  it('mostra o texto de um link, sem torná-lo navegável', () => {
    expect(readInlines('veja [a página](https://example.com) do release')).toEqual([
      { kind: 'text', text: 'veja ' },
      { kind: 'text', text: 'a página' },
      { kind: 'text', text: ' do release' },
    ]);
  });

  /** HTML que venha no release nunca vira marcação: continua sendo texto. */
  it('trata HTML como texto', () => {
    const notes = readNotes('<img src=x onerror=alert(1)>');
    expect(notes.blocks).toEqual([
      { kind: 'paragraph', inlines: [{ kind: 'text', text: '<img src=x onerror=alert(1)>' }] },
    ]);
  });

  it('lê listas numeradas, citações, código e separadores', () => {
    const notes = readNotes(
      [
        '1. primeiro',
        '2. segundo',
        '',
        '> uma citação',
        '',
        '```',
        'código cru',
        '```',
        '',
        '---',
      ].join('\n'),
    );
    expect(notes.blocks.map((block) => block.kind)).toEqual(['list', 'quote', 'code', 'rule']);
    expect(notes.blocks[0]).toMatchObject({ kind: 'list', ordered: true });
    expect(notes.blocks[2]).toEqual({ kind: 'code', text: 'código cru' });
  });

  it('um texto vazio não produz nada', () => {
    expect(readNotes('   \n\n')).toEqual({ title: null, blocks: [] });
  });
});
