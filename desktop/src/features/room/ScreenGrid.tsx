import { type CSSProperties, useCallback, useEffect, useRef } from 'react';
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
  const screens = useMediaStore((state) => state.screens);
  const solo = useMediaStore((state) => state.solo);
  const publishing = useMediaStore((state) => state.publishing);
  const focused = useMediaStore((state) => state.focused);
  const detached = useMediaStore((state) => state.detached);
  const participants = useRoomStore((state) => state.participants);
  const selfId = useSessionStore((state) => state.user?.id);
  const showSelfPreview = useUiStore((state) => state.showSelfPreview);
  const railWidth = useUiStore((state) => state.railWidth);
  const detachedWindow = useRef<DetachedWindow | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);

  const tiles = visibleTiles(screenOrder, screens, publishing, showSelfPreview);
  const layout = focused === null ? 'grid' : solo ? 'solo' : 'hybrid';

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
    const handle = detach(element, () => {
      home?.append(element);
      useMediaStore.getState().setDetached(null);
      detachedWindow.current = null;
    });
    if (handle === null) {
      useUiStore.getState().toast('warning', 'Não consegui abrir a janela destacada.');
      return;
    }
    detachedWindow.current = handle;
    useMediaStore.getState().setDetached(identity);
  }, []);

  if (tiles.length === 0) {
    return null;
  }

  return (
    <div
      ref={gridRef}
      className="relative screen-grid"
      data-count={focused === null ? Math.min(tiles.length, 6) : tiles.length}
      data-layout={layout}
      style={
        {
          '--rail-width': `${railWidth}%`,
          '--rail-count': Math.max(1, tiles.length - 1),
        } as CSSProperties
      }
    >
      {layout === 'hybrid' && tiles.length > 1 && <Split gridRef={gridRef} />}
      {tiles.map((identity) => (
        <ScreenTile
          key={identity}
          identity={identity}
          // O ladrilho da própria tela lê o participante que é a gente: é dele
          // que sai o `publishing_since` do servidor, e o relógio precisa ser o
          // mesmo que os outros estão vendo (RF-34).
          owner={participants[identity === SELF_ID ? (selfId ?? '') : identity]}
          focused={focused === identity}
          role={focused === null ? 'grid' : focused === identity ? 'main' : 'rail'}
          // No exclusivo as outras saem do documento, e não só da vista: oculto
          // é o que faz o `adaptiveStream` parar de baixar os quadros delas
          // (RF-32). Escondê-las com CSS continuaria pagando por todas.
          hidden={layout === 'solo' && focused !== identity}
          detached={detached === identity}
          onFocus={() => {
            useMediaStore.getState().focus(focused === identity ? null : identity);
          }}
          onDetach={() => {
            onDetach(identity);
          }}
          onFullscreen={onFullscreen}
        />
      ))}
    </div>
  );
}

/**
 * Arrasta a divisão entre a tela em foco e a coluna lateral (issue #7).
 *
 * Durante o gesto, a largura vai direto para a variável CSS do elemento: passar
 * por estado do React redesenharia a árvore do vídeo a cada movimento do
 * ponteiro, que é exatamente o trabalho por quadro que CLAUDE.md §7 proíbe no
 * caminho de render. O store só é escrito ao soltar, porque é ele que faz a
 * escolha sobreviver a sair e voltar do foco.
 */
function Split({ gridRef }: { gridRef: React.RefObject<HTMLDivElement | null> }) {
  const setRailWidth = useUiStore((state) => state.setRailWidth);
  const railWidth = useUiStore((state) => state.railWidth);

  const widthAt = (clientX: number): number | null => {
    const grid = gridRef.current;
    if (grid === null) {
      return null;
    }
    const box = grid.getBoundingClientRect();
    if (box.width === 0) {
      return null;
    }
    return Math.min(45, Math.max(10, ((box.right - clientX) / box.width) * 100));
  };

  return (
    <button
      type="button"
      className="screen-split z-20"
      style={{ left: `calc(100% - ${railWidth}%)` }}
      aria-label="Ajustar a largura da coluna lateral"
      aria-valuenow={railWidth}
      aria-valuemin={10}
      aria-valuemax={45}
      role="separator"
      aria-orientation="vertical"
      tabIndex={0}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (!event.currentTarget.hasPointerCapture(event.pointerId)) {
          return;
        }
        const width = widthAt(event.clientX);
        if (width !== null) {
          gridRef.current?.style.setProperty('--rail-width', `${width}%`);
          event.currentTarget.style.left = `calc(100% - ${width}%)`;
        }
      }}
      onPointerUp={(event) => {
        event.currentTarget.releasePointerCapture(event.pointerId);
        const width = widthAt(event.clientX);
        if (width !== null) {
          setRailWidth(width);
        }
      }}
      // Sem teclado, a divisão seria inalcançável para quem não usa mouse.
      onKeyDown={(event) => {
        const step = event.key === 'ArrowLeft' ? 2 : event.key === 'ArrowRight' ? -2 : 0;
        if (step !== 0) {
          event.preventDefault();
          setRailWidth(railWidth + step);
        }
      }}
    />
  );
}
