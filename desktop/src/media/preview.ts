import { log } from '../log';
import { onPreviewFrame } from './native';
import type { PublicationSource } from './publication';

/**
 * O preview da própria transmissão, do lado do WebView (ADR-0030).
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
 *
 * **Um por fonte** desde o ADR-0038: tela e câmera podem estar no ar ao mesmo
 * tempo, e os quadros chegam do core marcados com a fonte a que pertencem. Sem
 * isso, o quadro da câmera apareceria dentro do ladrilho da tela.
 */

interface Slot {
  element: HTMLImageElement | null;
  latest: string | null;
}

const slots: Record<PublicationSource, Slot> = {
  screen: { element: null, latest: null },
  camera: { element: null, latest: null },
};
let bridge: Promise<() => void> | null = null;

export function registerPreviewElement(
  source: PublicationSource,
  next: HTMLImageElement | null,
): void {
  const slot = slots[source];
  slot.element = next;
  if (next !== null && slot.latest !== null) {
    next.src = slot.latest;
  }
}

/** Assina o evento do core uma vez só, na subida do aplicativo. */
export function startPreviewBridge(): void {
  if (bridge !== null) {
    return;
  }
  bridge = onPreviewFrame((frame) => {
    const slot = slots[frame.source];
    slot.latest = frame.image;
    if (slot.element !== null) {
      slot.element.src = frame.image;
    }
  });
  void bridge.catch((error: unknown) => {
    log.error('preview: não consegui ouvir os quadros', error);
    bridge = null;
  });
}

/**
 * Esquece o último quadro de uma fonte. Chamado ao parar de transmitir: sem
 * isso, começar de novo mostraria por um instante a tela da sessão anterior —
 * que pode ser justamente a que não se queria mostrar.
 */
export function clearPreview(source: PublicationSource): void {
  const slot = slots[source];
  slot.latest = null;
  if (slot.element !== null) {
    // Um pixel transparente, e não `removeAttribute`: tirar o `src` de uma
    // `<img>` que já tinha um deixa o ícone de imagem quebrada no lugar.
    slot.element.src = BLANK;
  }
}

const BLANK = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';
