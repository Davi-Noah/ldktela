import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { PublishPreset } from '../store/media';
import type { PublicationSource } from './publication';

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
export type AudioMode = 'excluding_discord' | 'only_window' | 'whole_system';

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

/**
 * One source's thumbnail, as a data URL (RF-37).
 *
 * Asked for one at a time rather than with the list: capturing fifteen windows
 * costs close to a second, and the picker has to open now. `null` means the
 * source gave no frame — minimised, or closed between listing and capturing —
 * and the card falls back to its title, which still works.
 */
export function shareThumbnail(kind: SourceKind, sourceId: string): Promise<string | null> {
  return invoke<string | null>('share_thumbnail', { kind, sourceId });
}

/**
 * Turns the local preview of our own screen on and off, and sets its rate
 * (ADR-0030).
 *
 * Off stops the branch **inside the capture thread**. It is not the element
 * that disappears — it is the sub-sampling that stops happening.
 */
export function setSharePreview(enabled: boolean, fps: number, focused: boolean): Promise<void> {
  // `focused` escolhe a resolução, não só o relógio: em foco o preview enche a
  // janela, e a imagem pensada para ladrilho fica borrada ali (ADR-0030).
  return invoke<void>('share_preview', { enabled, fps, focused });
}

/**
 * Mirrors the sharing state onto the tray: the stop item, the tooltip and the
 * red dot on the icon.
 *
 * Swallows its own failure. The tray is a mirror of state that already exists
 * in the window, and a rejected promise here would be an unhandled rejection
 * over something purely cosmetic.
 */
export function setTraySharing(sharing: boolean, what: string | null): void {
  void invoke<void>('tray_set_sharing', { sharing, what }).catch(() => {
    // Sem bandeja, o aplicativo continua inteiro.
  });
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

/**
 * The tray item and the global hotkey (Ctrl+Shift+E) ask; this side decides.
 *
 * The core never stops a share on its own here because only this side knows
 * whether there is one, and it is this side that has to tell the server.
 */
export function onStopRequested(handler: () => void): Promise<() => void> {
  return listen('share://stop-requested', () => {
    handler();
  });
}

/**
 * One preview frame, already a data URL an `<img>` can take (ADR-0030).
 *
 * Carries the source since ADR-0038: com tela e câmera no ar ao mesmo tempo,
 * dois fluxos de quadros chegam por este mesmo evento.
 */
export interface PreviewFrame {
  source: PublicationSource;
  image: string;
}

export function onPreviewFrame(handler: (frame: PreviewFrame) => void): Promise<() => void> {
  return listen<PreviewFrame>('share://preview', (event) => {
    handler(event.payload);
  });
}

/** The cameras the core found (ADR-0038). Enumerated by Media Foundation. */
export interface CameraDevice {
  id: string;
  name: string;
}

export function listCameras(): Promise<CameraDevice[]> {
  return invoke<CameraDevice[]>('camera_list');
}

export interface StartCameraRequest {
  url: string;
  token: string;
  deviceId: string;
}

/**
 * Publishes the camera on the connection the core already holds, or opens one
 * when the screen is not sharing (ADR-0038).
 */
export function startNativeCamera(request: StartCameraRequest): Promise<void> {
  return invoke<void>('camera_start', {
    request: {
      url: request.url,
      token: request.token,
      device_id: request.deviceId,
    },
  });
}

export function stopNativeCamera(): Promise<void> {
  return invoke<void>('camera_stop');
}

/**
 * Turns the local camera preview on and off, exactly like the screen's.
 *
 * Espelhado só aqui, e nunca na trilha que sai: espelhar o que os outros veem
 * inverteria qualquer texto na frente da câmera (ADR-0038).
 */
export function setCameraPreview(enabled: boolean, fps: number, focused: boolean): Promise<void> {
  return invoke<void>('camera_preview', { enabled, fps, focused });
}
