import { useCallback, useEffect, useRef, useState } from 'react';
import { log } from '../../log';
import { detach, type DetachedWindow } from '../../media/pip';
import { SELF_ID, useMediaStore, visibleTiles } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { useSessionStore } from '../../store/session';
import { useUiStore } from '../../store/ui';
import { ScreenTile } from './ScreenTile';

/**
 * Every received screen, always mounted, in one stable container (RF-31), plus
 * our own preview when we are publishing (ADR-0030).
 *
 * Focus and detach change **CSS and parentage**, never the React tree position:
 * moving a tile between two containers would remount the `<video>` and cost
 * seconds of black frames on every click (CLAUDE.md §7).
 *
 * The number of columns is decided in CSS, by a container query over
 * `data-count` (`index.css`). A `ResizeObserver` next to the video render path
 * is exactly what CLAUDE.md §7 asks us not to write.
 */
export function ScreenGrid({ onFullscreen }: { onFullscreen: () => void }) {
  const screenOrder = useMediaStore((state) => state.screenOrder);
  const publishing = useMediaStore((state) => state.publishing);
  const focused = useMediaStore((state) => state.focused);
  const detached = useMediaStore((state) => state.detached);
  const participants = useRoomStore((state) => state.participants);
  const selfId = useSessionStore((state) => state.user?.id);
  const showSelfPreview = useUiStore((state) => state.showSelfPreview);
  const detachedWindow = useRef<DetachedWindow | null>(null);
  const [floating, setFloating] = useState<string | null>(null);

  const tiles = visibleTiles(screenOrder, publishing, showSelfPreview);

  // Fechar a janela destacada quando o componente sai, ou ela fica órfã na tela.
  useEffect(() => {
    return () => {
      detachedWindow.current?.close();
      detachedWindow.current = null;
    };
  }, []);

  const onDetach = useCallback((identity: string) => {
    const store = useMediaStore.getState();
    if (store.detached === identity) {
      detachedWindow.current?.close();
      detachedWindow.current = null;
      return;
    }
    // Uma janela por vez: é o limite do Document PiP (ADR-0022). Trocar em
    // silêncio faria o clique parecer sem efeito, então a troca é dita.
    if (detachedWindow.current !== null) {
      useUiStore
        .getState()
        .toast('info', 'Só uma tela por vez em janela destacada. A anterior foi fechada.');
    }
    detachedWindow.current?.close();
    detachedWindow.current = null;

    const element = document.querySelector<HTMLElement>(
      `[data-screen="${identity}"] [data-screen-media]`,
    );
    if (element === null) {
      return;
    }
    const home = element.parentElement;
    void detach(element, () => {
      home?.append(element);
      useMediaStore.getState().setDetached(null);
      detachedWindow.current = null;
    }).then((handle) => {
      if (handle === null) {
        useUiStore
          .getState()
          .toast('warning', 'Não consegui abrir a janela destacada nesta versão do Windows.');
        return;
      }
      detachedWindow.current = handle;
      useMediaStore.getState().setDetached(identity);
    });
  }, []);

  /**
   * A **segunda** janela flutuante (ADR-0022 continua valendo).
   *
   * Picture-in-Picture nativo é outra API, com outra janela, sobre a mesma
   * conexão do LiveKit: o elemento é o mesmo, o decodificador é o mesmo e não
   * há assinatura nova. Dá para ter uma tela destacada com controles e outra
   * flutuando ao mesmo tempo — que é mais do que o Discord faz.
   */
  const onFloat = useCallback((identity: string) => {
    const video = document.querySelector<HTMLVideoElement>(`[data-screen="${identity}"] video`);
    if (video === null) {
      return;
    }
    if (document.pictureInPictureElement === video) {
      void document.exitPictureInPicture().catch(() => {
        // Já saiu por conta própria; não há nada a fazer.
      });
      return;
    }
    video.addEventListener(
      'leavepictureinpicture',
      () => {
        setFloating(null);
      },
      { once: true },
    );
    video.requestPictureInPicture().then(
      () => {
        setFloating(identity);
      },
      (error: unknown) => {
        log.warn('flutuante: o WebView recusou', { error });
        useUiStore.getState().toast('warning', 'Não consegui abrir a janela flutuante.');
      },
    );
  }, []);

  if (tiles.length === 0) {
    return null;
  }

  return (
    <div className="screen-grid" data-count={Math.min(tiles.length, 6)}>
      {tiles.map((identity) => (
        <ScreenTile
          key={identity}
          identity={identity}
          // O ladrilho da própria tela lê o participante que é a gente: é dele
          // que sai o `publishing_since` do servidor, e o relógio precisa ser o
          // mesmo que os outros estão vendo (RF-34).
          owner={participants[identity === SELF_ID ? (selfId ?? '') : identity]}
          focused={focused === identity}
          hidden={focused !== null && focused !== identity}
          detached={detached === identity}
          floating={floating === identity}
          onFocus={() => {
            useMediaStore.getState().focus(focused === identity ? null : identity);
          }}
          onDetach={() => {
            onDetach(identity);
          }}
          onFloat={() => {
            onFloat(identity);
          }}
          onFullscreen={onFullscreen}
        />
      ))}
    </div>
  );
}
