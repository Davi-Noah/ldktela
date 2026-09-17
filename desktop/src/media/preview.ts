import { log } from '../log';
import { onPreviewFrame } from './native';

/**
 * O preview da própria tela, do lado do WebView (ADR-0030).
 *
 * Não passa pelo React. O `<img>` é criado imperativamente pelo ladrilho e
 * registrado aqui; a chegada de um quadro troca o `src` e nada mais. A regra é
 * a mesma do `<video>` (CLAUDE.md §7): um `setState` a cada quadro faria a
 * árvore inteira re-renderizar três a doze vezes por segundo, ao lado de um
 * decodificador de vídeo.
 *
 * O último quadro fica guardado para que remontar o elemento — trocar de foco,
 * destacar para outra janela — não deixe um retângulo vazio até o próximo
 * chegar.
 */

let element: HTMLImageElement | null = null;
let latest: string | null = null;
let bridge: Promise<() => void> | null = null;

export function registerPreviewElement(next: HTMLImageElement | null): void {
  element = next;
  if (next !== null && latest !== null) {
    next.src = latest;
  }
}

/** Assina o evento do core uma vez só, na subida do aplicativo. */
export function startPreviewBridge(): void {
  if (bridge !== null) {
    return;
  }
  bridge = onPreviewFrame((dataUrl) => {
    latest = dataUrl;
    if (element !== null) {
      element.src = dataUrl;
    }
  });
  void bridge.catch((error: unknown) => {
    log.error('preview: não consegui ouvir os quadros', error);
    bridge = null;
  });
}

/**
 * Esquece o último quadro. Chamado ao parar de compartilhar: sem isso, começar
 * de novo mostraria por um instante a tela da sessão anterior — que pode ser
 * justamente a que não se queria mostrar.
 */
export function clearPreview(): void {
  latest = null;
  if (element !== null) {
    // Um pixel transparente, e não `removeAttribute`: tirar o `src` de uma
    // `<img>` que já tinha um deixa o ícone de imagem quebrada no lugar.
    element.src = BLANK;
  }
}

const BLANK = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';
