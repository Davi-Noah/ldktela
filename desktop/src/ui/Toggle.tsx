interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  describedBy?: string;
  disabled?: boolean;
}

/**
 * Interruptor. Substitui `<input type="checkbox">`, que o Windows desenha com a
 * própria caixa e não aceita tema nenhum.
 */
export function Toggle({ checked, onChange, label, describedBy, disabled = false }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-describedby={describedBy}
      disabled={disabled}
      onClick={() => {
        onChange(!checked);
      }}
      className="flex items-center gap-2 text-left text-text disabled:cursor-not-allowed disabled:text-text-faint"
    >
      <span
        aria-hidden="true"
        className={`relative inline-flex h-5 w-9 shrink-0 rounded-pill ${
          checked ? 'bg-accent' : 'bg-surface-3'
        }`}
      >
        <span
          className={`absolute top-0.5 h-4 w-4 rounded-pill bg-surface-0 ${
            checked ? 'left-4.5' : 'left-0.5'
          }`}
        />
      </span>
      <span>{label}</span>
    </button>
  );
}
