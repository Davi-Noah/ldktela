import type { ButtonHTMLAttributes } from 'react';

type Variant = 'primary' | 'neutral' | 'danger';

const VARIANTS: Record<Variant, string> = {
  primary: 'bg-accent text-surface-0 hover:brightness-110 disabled:brightness-75',
  neutral: 'bg-surface-2 text-text hover:bg-surface-3 disabled:text-text-faint',
  danger: 'bg-surface-2 text-danger hover:bg-surface-3 disabled:text-text-faint',
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
}

export function Button({ variant = 'neutral', className, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      className={`rounded-panel px-3 py-1.5 font-medium disabled:cursor-not-allowed ${VARIANTS[variant]} ${className ?? ''}`}
    />
  );
}
