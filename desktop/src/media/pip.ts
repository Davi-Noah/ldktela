/**
 * Detaching a screen into its own OS window (RF-33, ADR-0022).
 *
 * Document Picture-in-Picture gives a real, movable, always-on-top window that
 * **shares this JavaScript context**. That is the whole reason it was chosen over
 * extra Tauri windows: the `<video>` element is *moved* into it, so the track
 * stays subscribed on the same connection and the decoder is never torn down
 * (CLAUDE.md §7). A second Tauri window would need a second LiveKit connection,
 * a suffixed identity and a change to `room_presence`.
 */

import { log } from '../log';

interface DocumentPictureInPictureApi {
  requestWindow: (options?: { width?: number; height?: number }) => Promise<Window>;
  readonly window: Window | null;
}

function api(): DocumentPictureInPictureApi | null {
  const candidate = (window as unknown as Record<string, unknown>).documentPictureInPicture;
  return candidate === undefined ? null : (candidate as DocumentPictureInPictureApi);
}

export function pictureInPictureSupported(): boolean {
  return api() !== null;
}

export interface DetachedWindow {
  close: () => void;
}

/**
 * Opens the window and moves `element` into it. `onClose` runs whether the user
 * closed the window or the caller did, and must put the element back.
 */
export async function detach(
  element: HTMLElement,
  onClose: () => void,
): Promise<DetachedWindow | null> {
  const pip = api();
  if (pip === null) {
    log.warn('destacar: Document Picture-in-Picture indisponível neste WebView');
    return null;
  }

  let target: Window;
  try {
    target = await pip.requestWindow({
      width: Math.max(640, element.clientWidth),
      height: Math.max(360, element.clientHeight),
    });
  } catch (error) {
    // Negado por falta de gesto do usuário, ou já há uma janela aberta.
    log.error('destacar: não consegui abrir a janela', error);
    return null;
  }

  copyStyles(target);
  target.document.body.style.margin = '0';
  target.document.body.style.background = '#000';
  target.document.body.append(element);

  const handleUnload = () => {
    onClose();
  };
  target.addEventListener('pagehide', handleUnload, { once: true });

  log.info('destacar: janela aberta');
  return {
    close: () => {
      target.removeEventListener('pagehide', handleUnload);
      target.close();
      onClose();
    },
  };
}

/**
 * The PiP window starts with no styles at all — it is a separate document. Tailwind
 * lives in `<style>` tags injected by Vite in dev and in a `<link>` in a build, so
 * both have to be carried across or the detached screen renders unstyled.
 */
function copyStyles(target: Window): void {
  for (const sheet of Array.from(document.styleSheets)) {
    try {
      const text = Array.from(sheet.cssRules)
        .map((rule) => rule.cssText)
        .join('');
      const style = target.document.createElement('style');
      style.textContent = text;
      target.document.head.append(style);
    } catch {
      // Folha de outra origem: não dá para ler as regras, então copia-se o link.
      const href = sheet.href;
      if (href !== null) {
        const link = target.document.createElement('link');
        link.rel = 'stylesheet';
        link.href = href;
        target.document.head.append(link);
      }
    }
  }
}
