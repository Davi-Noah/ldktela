import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { PublishPreset } from '../store/media';

/**
 * The publishing half of the client, which lives in the Rust core (ADR-0026).
 *
 * Nothing here acquires media in the WebView. `getDisplayMedia` is gone, and
 * with it Chromium's source picker and its "you are sharing" bar — they were
 * never removable, only avoidable, by not asking the browser for a display.
 */

export type SourceKind = 'screen' | 'window';

export interface ShareSource {
  /** Opaque. On Windows a window's id is its `HWND`, hence a string: a JSON
      number would round it. */
  id: string;
  kind: SourceKind;
  title: string;
}

/** Which of the two capture modes the core actually got (RF-30). */
export type AudioMode = 'excluding_discord' | 'whole_system';

export interface StartedShare {
  /** `null` when sharing without audio. */
  audio: AudioMode | null;
}

export interface StartShareRequest {
  url: string;
  token: string;
  sourceId: string;
  kind: SourceKind;
  preset: PublishPreset;
  audio: boolean;
}

export function listShareSources(): Promise<ShareSource[]> {
  return invoke<ShareSource[]>('share_sources');
}

export function startNativeShare(request: StartShareRequest): Promise<StartedShare> {
  // O core e um motor de midia burro: recebe URL, token, fonte e preset. Quem
  // fala com a nossa API e resolve autenticacao continua sendo este lado.
  return invoke<StartedShare>('share_start', {
    request: {
      url: request.url,
      token: request.token,
      source_id: request.sourceId,
      kind: request.kind,
      preset: request.preset,
      audio: request.audio,
    },
  });
}

export function stopNativeShare(): Promise<void> {
  return invoke<void>('share_stop');
}

/**
 * Fires when a share ends without the user asking: the SFU dropped the
 * publishing connection, or the window being shared was closed. Without it the
 * button would keep saying "stop sharing" over a stream nobody is receiving.
 */
export function onShareEnded(handler: (reason: string) => void): Promise<() => void> {
  return listen<string>('share://ended', (event) => {
    handler(event.payload);
  });
}
