import type { ButtonHTMLAttributes } from 'react';
import { Icon, type IconName } from './Icon';

type Variant = 'chrome' | 'ghost' | 'danger';

const VARIANTS: Record<Variant, string> = {
  // Sobre o vídeo: fundo próprio, porque um ícone sem superfície some em
  // qualquer imagem clara.
  chrome:
    'bg-surface-2/85 text-text hover:bg-surface-3 aria-pressed:bg-accent aria-pressed:text-surface-0',
  ghost: 'text-text-muted hover:bg-surface-2 hover:text-text aria-pressed:text-accent',
  danger: 'bg-danger-strong text-text hover:brightness-110',
};

interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  icon: IconName;
  /** Vira `aria-label` e a dica: um botão só de ícone não pode ter só o desenho. */
  label: string;
  variant?: Variant;
  size?: number;
  /** Onde a dica aparece. Em cromo colado no rodapé, só cabe para cima. */
  tipSide?: 'top' | 'bottom';
}

export function IconButton({
  icon,
  label,
  variant = 'chrome',
  size = 18,
  tipSide = 'top',
  className,
  ...rest
}: IconButtonProps) {
  return (
    <span className="group relative inline-flex">
      <button
        type="button"
        aria-label={label}
        {...rest}
        className={`inline-flex items-center justify-center rounded-pill p-2 disabled:cursor-not-allowed disabled:opacity-40 ${VARIANTS[variant]} ${className ?? ''}`}
      >
        <Icon name={icon} size={size} />
      </button>
      <span
        role="tooltip"
        className={`chrome-fade pointer-events-none absolute left-1/2 z-50 -translate-x-1/2 whitespace-nowrap rounded-panel border border-border bg-surface-0 px-2 py-1 text-xs text-text opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 ${
          tipSide === 'top' ? 'bottom-full mb-1.5' : 'top-full mt-1.5'
        }`}
      >
        {label}
      </span>
    </span>
  );
}
