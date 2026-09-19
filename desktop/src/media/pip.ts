/**
 * Detaching a screen into its own OS window (RF-33, ADR-0022, ADR-0034).
 *
 * The window **shares this JavaScript context**. That is the whole reason it is
 * not a second Tauri window: the `<video>` element is *moved* into it, so the
 * track stays subscribed on the same connection and the decoder is never torn
 * down (CLAUDE.md §7). A second Tauri window would need a second LiveKit
 * connection, a suffixed identity and a change to `room_presence`.
 */

import { log } from '../log';

export interface DetachedWindow {
  close: () => void;
}

/** O nome é fixo: um segundo `window.open` com ele reaproveita a janela. */
const WINDOW_NAME = 'ldktela-destacada';

/**
 * Opens the window and moves `element` into it. `onClose` runs whether the user
 * closed the window or the caller did, and must put the element back.
 *
 * The window is a plain same-origin popup, not Document Picture-in-Picture
 * (ADR-0034). WebView2 exposes `documentPictureInPicture`, and every call to it
 * fails with `InvalidStateError: Internal error: no window` — the host never
 * creates that window. A popup opened by this document is still this JavaScript
 * context, so the `<video>` is *moved* exactly as ADR-0022 intended: same track,
 * same connection, same decoder. The Rust side allows `about:blank` and nothing
 * else.
 */
export function detach(element: HTMLElement, onClose: () => void): DetachedWindow | null {
  const width = Math.max(640, element.clientWidth);
  const height = Math.max(360, element.clientHeight);
  const target = window.open('about:blank', WINDOW_NAME, `popup,width=${width},height=${height}`);
  if (target === null) {
    log.warn('destacar: o WebView recusou a janela');
    return null;
  }

  target.document.title = 'ldktela';
  copyStyles(target);
  const { documentElement, body } = target.document;
  documentElement.style.height = '100%';
  body.style.height = '100%';
  body.style.margin = '0';
  body.style.background = '#000';
  // O elemento chega de um ladrilho que lhe dava altura; aqui não há grade
  // nenhuma, e sem isto ele colapsaria para zero e a janela abriria preta.
  element.style.height = '100%';
  element.style.width = '100%';
  body.append(element);

  let done = false;
  const finish = () => {
    if (done) {
      return;
    }
    done = true;
    window.clearInterval(watch);
    element.style.removeProperty('height');
    element.style.removeProperty('width');
    onClose();
  };
  target.addEventListener('pagehide', finish, { once: true });
  // `pagehide` de um popup nem sempre chega a quem o abriu quando o usuário
  // fecha pelo X. Conferir `closed` a cada meio segundo é o que garante que o
  // vídeo volta para o ladrilho em vez de sumir junto com a janela.
  const watch = window.setInterval(() => {
    if (target.closed) {
      finish();
    }
  }, 500);

  log.info('destacar: janela aberta');
  return {
    close: () => {
      target.removeEventListener('pagehide', finish);
      finish();
      target.close();
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
