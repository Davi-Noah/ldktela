import { useUiStore } from '../store/ui';
import { Icon, type IconName } from './Icon';
import { IconButton } from './IconButton';

const TONE: Record<string, { icon: IconName; color: string }> = {
  info: { icon: 'info', color: 'text-text-muted' },
  warning: { icon: 'alert', color: 'text-warning' },
  danger: { icon: 'alert', color: 'text-danger' },
};

/**
 * Onde todo aviso do aplicativo aparece.
 *
 * Antes disto, o erro de compartilhamento era um `<p>` dentro do corpo da sala —
 * que só é montado quando **não** há vídeo na tela. Falhar ao compartilhar
 * enquanto se assistia a tela de alguém produzia um erro que ninguém via.
 */
export function Toasts() {
  const toasts = useUiStore((state) => state.toasts);
  const dismiss = useUiStore((state) => state.dismiss);

  if (toasts.length === 0) {
    return null;
  }

  return (
    <div
      role="status"
      aria-live="polite"
      className="pointer-events-none fixed inset-x-0 top-3 z-50 flex flex-col items-center gap-2 px-4"
    >
      {toasts.map((toast) => {
        const tone = TONE[toast.tone] ?? TONE.info;
        return (
          <div
            key={toast.id}
            className="pointer-events-auto flex max-w-lg items-start gap-2 rounded-panel border border-border bg-surface-1 py-2 pl-3 pr-1"
          >
            <span className={`mt-0.5 shrink-0 ${tone?.color ?? ''}`}>
              <Icon name={tone?.icon ?? 'info'} size={16} />
            </span>
            <p className="selectable flex-1 text-text">{toast.text}</p>
            <IconButton
              icon="close"
              label="Dispensar"
              variant="ghost"
              size={14}
              tipSide="bottom"
              onClick={() => {
                dismiss(toast.id);
              }}
            />
          </div>
        );
      })}
    </div>
  );
}
