import { useCallback, useEffect, useRef } from 'react';
import { detach, type DetachedWindow } from '../../media/pip';
import { useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { ScreenTile } from './ScreenTile';

/**
 * Every received screen, always mounted, in one stable container (RF-31).
 *
 * Focus and detach change **CSS and parentage**, never the React tree position:
 * moving a tile between two containers would remount the `<video>` and cost
 * seconds of black frames on every click (CLAUDE.md §7).
 */
export function ScreenGrid() {
  const screenOrder = useMediaStore((state) => state.screenOrder);
  const focused = useMediaStore((state) => state.focused);
  const detached = useMediaStore((state) => state.detached);
  const participants = useRoomStore((state) => state.participants);
  const detachedWindow = useRef<DetachedWindow | null>(null);

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
    // Uma janela por vez: é o limite do Document PiP (ADR-0022).
    detachedWindow.current?.close();
    detachedWindow.current = null;

    const element = document.querySelector<HTMLElement>(`[data-screen="${identity}"]`);
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
        return;
      }
      detachedWindow.current = handle;
      useMediaStore.getState().setDetached(identity);
    });
  }, []);

  if (screenOrder.length === 0) {
    return null;
  }

  const columns = screenOrder.length === 1 ? 1 : 2;

  return (
    <div
      className="absolute inset-0 grid gap-2 p-2"
      style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
    >
      {screenOrder.map((identity) => (
        <ScreenTile
          key={identity}
          identity={identity}
          owner={participants[identity]}
          focused={focused === identity}
          hidden={focused !== null && focused !== identity}
          detached={detached === identity}
          onFocus={() => {
            useMediaStore.getState().focus(focused === identity ? null : identity);
          }}
          onDetach={() => {
            onDetach(identity);
          }}
        />
      ))}
    </div>
  );
}
