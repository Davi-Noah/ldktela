import type { ChangeEvent } from 'react';

interface SliderProps {
  value: number;
  onChange: (value: number) => void;
  label: string;
  disabled?: boolean;
  className?: string;
}

/**
 * Intervalo de 0 a 100.
 *
 * Continua sendo o `<input type="range">` nativo — teclado, roda do mouse e
 * leitor de tela vêm de graça e reescrever isso seria trabalho para piorar. O
 * que muda é a pintura, em `index.css`: `accent-color` sozinho deixa o trilho
 * com a cara do Windows.
 */
export function Slider({ value, onChange, label, disabled = false, className }: SliderProps) {
  return (
    <input
      type="range"
      min={0}
      max={100}
      value={value}
      disabled={disabled}
      aria-label={label}
      // O preenchimento do trilho é uma variável CSS: sem ela o gradiente não
      // tem como saber onde parar, e a barra ficaria cheia sempre.
      style={{ '--fill': `${value}%` } as React.CSSProperties}
      onChange={(event: ChangeEvent<HTMLInputElement>) => {
        onChange(Number(event.target.value));
      }}
      className={`slider ${className ?? ''}`}
    />
  );
}
