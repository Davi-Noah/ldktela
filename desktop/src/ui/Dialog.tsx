import { type ReactNode, useEffect, useId, useRef } from 'react';
import { IconButton } from './IconButton';

interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  /** Largura máxima do painel, em classe do Tailwind. */
  width?: string;
}

const FOCUSABLE =
  'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

/**
 * Diálogo modal de verdade: `role="dialog"`, `aria-modal`, Escape, clique no
 * fundo e foco preso dentro do painel.
 *
 * O seletor de tela era uma `div` sobre outra `div`. Sem Escape, sem foco preso
 * e sem Enter — com a primeira fonte já selecionada, o usuário apertava Enter e
 * nada acontecia.
 */
export function Dialog({ title, onClose, children, footer, width = 'max-w-3xl' }: DialogProps) {
  const panel = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    const previous = document.activeElement;
    panel.current?.querySelector<HTMLElement>(FOCUSABLE)?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onClose();
        return;
      }
      if (event.key !== 'Tab') {
        return;
      }
      const targets = Array.from(panel.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []);
      if (targets.length === 0) {
        return;
      }
      const first = targets[0];
      const last = targets[targets.length - 1];
      if (first === undefined || last === undefined) {
        return;
      }
      // Sem isto o Tab sai do diálogo e vai passear pelo cromo da sala que está
      // atrás dele, que é exatamente o que um modal existe para impedir.
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      document.removeEventListener('keydown', onKeyDown, true);
      if (previous instanceof HTMLElement) {
        previous.focus();
      }
    };
  }, [onClose]);

  return (
    <div
      className="absolute inset-0 z-40 flex items-center justify-center bg-scrim p-6"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className={`flex max-h-full w-full ${width} flex-col overflow-hidden rounded-panel border border-border bg-surface-1`}
      >
        <header className="flex items-center gap-2 border-b border-line px-4 py-3">
          <h2 id={titleId} className="flex-1 font-semibold text-text">
            {title}
          </h2>
          <IconButton
            icon="close"
            label="Fechar"
            variant="ghost"
            size={16}
            tipSide="bottom"
            onClick={onClose}
          />
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">{children}</div>

        {footer !== undefined && (
          <footer className="border-t border-line bg-surface-1 px-4 py-3">{footer}</footer>
        )}
      </div>
    </div>
  );
}
