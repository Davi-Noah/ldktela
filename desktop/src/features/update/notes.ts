/**
 * As novidades de uma versão: quando mostrar, e como ler o texto (ADR-0040).
 *
 * Tudo aqui é puro. A rede e o disco ficam em `platform/releaseNotes.ts`, e o
 * que sobra é a parte que dá para provar sem nenhum dos dois.
 */

/** O que fazer com as novidades da versão que acabou de abrir. */
export type NotesVerdict =
  /** Atualizou: buscar o texto e mostrar. */
  | 'show'
  /** Não há o que mostrar, mas a versão precisa ficar registrada. */
  | 'record'
  /** Já registrada. */
  | 'nothing';

/**
 * Compara duas versões `x.y.z` número a número.
 *
 * `null` quando uma delas não tem esse formato: um registro estragado não é
 * motivo para adivinhar se houve atualização.
 */
export function compareVersions(a: string, b: string): number | null {
  const parse = (text: string) => {
    const parts = text.split('.');
    if (parts.length !== 3 || parts.some((part) => !/^\d+$/.test(part))) {
      return null;
    }
    return parts.map(Number);
  };
  const left = parse(a);
  const right = parse(b);
  if (left === null || right === null) {
    return null;
  }
  for (let index = 0; index < 3; index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) {
      return Math.sign(difference);
    }
  }
  return 0;
}

export function notesVerdict(options: {
  /** A última versão cujas novidades foram vistas, ou `null` se nunca houve registro. */
  seen: string | null;
  current: string;
  /**
   * Havia refresh token no cofre ao abrir. Sem registro, é a única pista de
   * que isto é uma atualização e não uma instalação nova (ADR-0040, decisão 3).
   */
  hadSession: boolean;
}): NotesVerdict {
  const { seen, current, hadSession } = options;
  if (seen === current) {
    return 'nothing';
  }
  if (seen === null) {
    return hadSession ? 'show' : 'record';
  }
  // Voltar para uma versão mais velha não é novidade, e um registro que não
  // se lê não prova atualização nenhuma.
  return compareVersions(seen, current) === -1 ? 'show' : 'record';
}

// ---------------------------------------------------------------------------
// O texto
// ---------------------------------------------------------------------------

export type Inline =
  | { kind: 'text'; text: string }
  | { kind: 'strong'; text: string }
  | { kind: 'em'; text: string }
  | { kind: 'code'; text: string };

export type Block =
  | { kind: 'heading'; level: 1 | 2 | 3; inlines: Inline[] }
  | { kind: 'paragraph'; inlines: Inline[] }
  | { kind: 'list'; ordered: boolean; items: Inline[][] }
  | { kind: 'quote'; inlines: Inline[] }
  | { kind: 'code'; text: string }
  | { kind: 'rule' };

export interface ReadNotes {
  /** O `# título` da primeira linha, se houver: ele vira o título do modal. */
  title: string | null;
  blocks: Block[];
}

/**
 * Código em linha, negrito, itálico com `*`, e link — nessa ordem, porque o
 * código protege o que estiver dentro dele. Itálico com `_` fica de fora: ele
 * morderia `screen_share_audio` no meio de uma nota técnica.
 */
const INLINE = /`([^`]+)`|\*\*([^*]+)\*\*|\[([^\]]+)\]\([^)\s]+\)|\*([^*\s](?:[^*]*[^*\s])?)\*/g;

export function readInlines(text: string): Inline[] {
  const inlines: Inline[] = [];
  let last = 0;
  for (const match of text.matchAll(INLINE)) {
    const at = match.index;
    if (at > last) {
      inlines.push({ kind: 'text', text: text.slice(last, at) });
    }
    const [, code, strong, link, em] = match;
    if (code !== undefined) {
      inlines.push({ kind: 'code', text: code });
    } else if (strong !== undefined) {
      inlines.push({ kind: 'strong', text: strong });
    } else if (link !== undefined) {
      // O aplicativo não abre navegador daqui, e navegar o WebView para fora
      // derrubaria a sala. O link aparece como o texto dele.
      inlines.push({ kind: 'text', text: link });
    } else if (em !== undefined) {
      inlines.push({ kind: 'em', text: em });
    }
    last = at + match[0].length;
  }
  if (last < text.length) {
    inlines.push({ kind: 'text', text: text.slice(last) });
  }
  return inlines;
}

const BULLET = /^\s{0,3}[-*+]\s+(.*)$/;
const NUMBERED = /^\s{0,3}\d+[.)]\s+(.*)$/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const FENCE = /^\s{0,3}```/;
const QUOTE = /^\s{0,3}>\s?(.*)$/;

/**
 * Lê o subconjunto de markdown que as notas usam, em blocos.
 *
 * Nunca produz HTML: quem desenha são elementos React, então nada do que vier
 * da rede vira marcação no WebView (ADR-0040, decisão 5). O que não se
 * reconhece vira parágrafo, com o texto como veio.
 */
export function readNotes(markdown: string): ReadNotes {
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  let list: { ordered: boolean; items: string[] } | null = null;
  let quote: string[] = [];

  const flush = () => {
    if (paragraph.length > 0) {
      blocks.push({ kind: 'paragraph', inlines: readInlines(paragraph.join(' ')) });
      paragraph = [];
    }
    if (list !== null) {
      blocks.push({ kind: 'list', ordered: list.ordered, items: list.items.map(readInlines) });
      list = null;
    }
    if (quote.length > 0) {
      blocks.push({ kind: 'quote', inlines: readInlines(quote.join(' ')) });
      quote = [];
    }
  };

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? '';

    if (FENCE.test(line)) {
      flush();
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !FENCE.test(lines[index] ?? '')) {
        code.push(lines[index] ?? '');
        index += 1;
      }
      blocks.push({ kind: 'code', text: code.join('\n') });
      continue;
    }

    if (line.trim() === '') {
      flush();
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading !== null) {
      flush();
      const depth = heading[1]?.length ?? 1;
      const level = (depth >= 3 ? 3 : depth) as 1 | 2 | 3;
      blocks.push({ kind: 'heading', level, inlines: readInlines(heading[2] ?? '') });
      continue;
    }

    if (RULE.test(line)) {
      flush();
      blocks.push({ kind: 'rule' });
      continue;
    }

    const quoted = QUOTE.exec(line);
    if (quoted !== null) {
      if (paragraph.length > 0 || list !== null) {
        flush();
      }
      quote.push(quoted[1] ?? '');
      continue;
    }

    const bullet = BULLET.exec(line);
    const numbered = bullet === null ? NUMBERED.exec(line) : null;
    const item = bullet ?? numbered;
    if (item !== null) {
      const ordered = numbered !== null;
      if (list === null || list.ordered !== ordered) {
        flush();
        list = { ordered, items: [] };
      }
      list.items.push(item[1] ?? '');
      continue;
    }

    // Linha recuada logo depois de um item: continuação dele, não parágrafo.
    if (list !== null && /^\s+\S/.test(line)) {
      const items = list.items;
      items[items.length - 1] = `${items[items.length - 1] ?? ''} ${line.trim()}`;
      continue;
    }

    if (list !== null || quote.length > 0) {
      flush();
    }
    paragraph.push(line.trim());
  }
  flush();

  // O título do release vira o título do modal, em vez de aparecer duas vezes.
  const [first, ...rest] = blocks;
  if (first?.kind === 'heading' && first.level === 1) {
    return { title: first.inlines.map((inline) => inline.text).join(''), blocks: rest };
  }
  return { title: null, blocks };
}
