import type { ButtonHTMLAttributes } from 'react';
import { Icon, type IconName } from './Icon';

type Variant = 'primary' | 'neutral' | 'danger' | 'subtle';

const VARIANTS: Record<Variant, string> = {
  primary: 'bg-accent text-surface-0 hover:brightness-110 disabled:brightness-75',
  neutral: 'bg-surface-2 text-text hover:bg-surface-3 disabled:text-text-faint',
  danger: 'bg-danger-strong text-text hover:brightness-110 disabled:brightness-75',
  subtle: 'text-text-muted hover:bg-surface-2 hover:text-text',
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  icon?: IconName;
}

export function Button({ variant = 'neutral', icon, className, children, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      className={`inline-flex items-center justify-center gap-1.5 rounded-panel px-3 py-1.5 font-medium disabled:cursor-not-allowed ${VARIANTS[variant]} ${className ?? ''}`}
    >
      {icon !== undefined && <Icon name={icon} size={16} />}
      {children}
    </button>
  );
}
