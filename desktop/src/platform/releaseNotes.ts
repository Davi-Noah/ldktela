import { invoke } from '@tauri-apps/api/core';
import { RELEASE_NOTES_TIMEOUT_MS, RELEASES_API } from '../config';

/**
 * As novidades de uma versão: o registro no core e o texto no GitHub (ADR-0040).
 *
 * A decisão de mostrar ou não mora em `features/update/notes.ts`, que é puro.
 * Aqui fica só o que toca disco e rede.
 */

/** A última versão cujas novidades foram vistas; `null` se nunca houve registro. */
export function readSeenVersion(): Promise<string | null> {
  return invoke<string | null>('release_notes_seen');
}

export function markVersionSeen(version: string): Promise<void> {
  return invoke<void>('release_notes_mark_seen', { version });
}

/** O texto do release publicado para esta versão. */
export interface ReleaseText {
  version: string;
  markdown: string;
}

/**
 * Busca o release publicado da tag `v{versão}`.
 *
 * `null` quando não há o que mostrar: a versão não tem release (build de
 * desenvolvimento) ou o release não tem texto. **Lança** quando não deu para
 * saber — sem rede, GitHub fora do ar, limite de pedidos —, porque aí a versão
 * não pode ser marcada como vista: a próxima abertura tenta de novo.
 */
export async function fetchReleaseText(version: string): Promise<ReleaseText | null> {
  const abort = new AbortController();
  const timer = setTimeout(() => {
    abort.abort();
  }, RELEASE_NOTES_TIMEOUT_MS);
  try {
    const response = await fetch(`${RELEASES_API}/tags/v${encodeURIComponent(version)}`, {
      headers: { Accept: 'application/vnd.github+json' },
      signal: abort.signal,
    });
    if (response.status === 404) {
      return null;
    }
    if (!response.ok) {
      throw new Error(`GitHub respondeu ${String(response.status)}`);
    }
    const payload: unknown = await response.json();
    const body =
      typeof payload === 'object' && payload !== null && 'body' in payload ? payload.body : null;
    if (typeof body !== 'string' || body.trim() === '') {
      return null;
    }
    return { version, markdown: body };
  } finally {
    clearTimeout(timer);
  }
}
