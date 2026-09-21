import { Icon } from '../../ui/Icon';

interface WatchButtonProps {
  /** Se esta tela está sendo assistida agora. */
  watching: boolean;
  onClick: () => void;
  /**
   * `md` na tela de entrada, onde este é o único gesto de cada linha; `sm` na
   * lista do popover, que já é densa. O desenho é o mesmo, e é isso que importa:
   * o mesmo gesto tem a mesma cara nos dois lugares.
   */
  size?: 'sm' | 'md';
}

/**
 * Entrar ou sair da tela de alguém (ADR-0036).
 *
 * Uma pílula, e não um `Button`: numa linha de lista o botão cheio pesava mais
 * que o nome da pessoa e brigava com a etiqueta ao lado. Entrar é o gesto que
 * convida, então leva o destaque; sair é o que se faz sem cerimônia, então fica
 * discreto até o ponteiro chegar.
 */
export function WatchButton({ watching, onClick, size = 'sm' }: WatchButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex shrink-0 items-center rounded-pill font-medium ${
        size === 'md' ? 'gap-1.5 px-3 py-1 text-sm' : 'gap-1 px-2 py-0.5 text-xs'
      } ${
        watching
          ? 'text-text-muted hover:bg-surface-3 hover:text-text'
          : 'bg-accent-soft text-text hover:bg-accent/30'
      }`}
    >
      <Icon name={watching ? 'eye-off' : 'eye'} size={size === 'md' ? 16 : 14} />
      {watching ? 'Sair' : 'Entrar'}
    </button>
  );
}
