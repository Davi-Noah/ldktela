import { type ReactNode, useEffect, useId, useRef, useState } from 'react';
import { Icon, type IconName } from './Icon';
import { useUiStore } from '../store/ui';

const ALIGNMENT: Record<'left' | 'right' | 'bar', string> = {
  left: 'left-0',
  right: 'right-0',
  bar: 'left-1/2 -translate-x-1/2',
};

interface PopoverProps {
  icon: IconName;
  label: string;
  children: (close: () => void) => ReactNode;
  /**
   * A que o painel se alinha. `bar` o centraliza na **barra** que contém o
   * botão, e não no botão: um painel largo presilhado a um ícone que fica na
   * ponta da barra nasce torto, e anda de lugar toda vez que a barra ganha ou
   * perde um controle.
   */
  align?: 'left' | 'right' | 'bar';
  /**
   * `group` para conteúdo que só informa. Um `menu` promete itens acionáveis a
   * quem navega por leitor de tela, e uma lista de nomes não tem nenhum.
   */
  role?: 'menu' | 'group';
  /**
   * Largura fixa, para conteúdo que tem colunas. `w-max` serve a um menu de
   * rótulos curtos; uma lista de nomes com ação ao lado precisa de um limite,
   * ou o item mais largo decide o tamanho do menu inteiro.
   */
  width?: string;
}

/**
 * Menu ancorado num botão de ícone.
 *
 * Existe porque `<select>` sobre vídeo abre o popup do Windows — fundo claro,
 * fonte do sistema, sem tema — e não há CSS que conserte isso.
 *
 * Enquanto está aberto, **segura o cromo**: sem isso o temporizador de
 * ociosidade esconderia a barra por baixo do próprio menu que o usuário abriu.
 */
export function Popover({
  icon,
  label,
  children,
  align = 'right',
  role = 'menu',
  width = 'w-max min-w-44',
}: PopoverProps) {
  const [open, setOpen] = useState(false);
  const holdChrome = useUiStore((state) => state.holdChrome);
  const anchor = useRef<HTMLDivElement>(null);
  const id = useId();

  useEffect(() => {
    if (!open) {
      return;
    }
    const release = holdChrome();
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || anchor.current?.contains(event.target) !== true) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        // Fechar o menu não pode significar sair do foco da tela: quem consome
        // o Escape primeiro é quem está por cima.
        event.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      release();
      document.removeEventListener('pointerdown', onPointerDown, true);
      document.removeEventListener('keydown', onKeyDown, true);
    };
  }, [open, holdChrome]);

  return (
    // Sem `relative` no modo `bar`: é o ancestral posicionado — a própria
    // barra — que o painel usa como referência para se centralizar.
    <div ref={anchor} className={align === 'bar' ? 'inline-flex' : 'relative inline-flex'}>
      <button
        type="button"
        aria-label={label}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => {
          setOpen((current) => !current);
        }}
        className="inline-flex items-center justify-center rounded-pill bg-surface-2/85 p-2 text-text hover:bg-surface-3 aria-expanded:bg-surface-3"
      >
        <Icon name={icon} size={18} />
      </button>
      {open && (
        <div
          id={id}
          role={role}
          aria-label={role === 'group' ? label : undefined}
          // `w-max`: um bloco `absolute` dentro de um âncora de 34 px calcula a
          // própria largura a partir desses 34 px, trava no `min-w-44`, e o
          // rótulo — que é o que trunca — perde para a dica ao lado: o menu de
          // qualidade mostrava "A…" no lugar de "Automático" (issue #4).
          className={`absolute bottom-full z-50 mb-2 max-w-[calc(100vw-1.5rem)] rounded-panel border border-border bg-surface-1 p-1 ${width} ${ALIGNMENT[align]}`}
        >
          {children(() => {
            setOpen(false);
          })}
        </div>
      )}
    </div>
  );
}

interface MenuItemProps {
  children: ReactNode;
  selected?: boolean;
  hint?: string;
  onClick: () => void;
}

export function MenuItem({ children, selected = false, hint, onClick }: MenuItemProps) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      onClick={onClick}
      className="flex w-full items-center gap-2 rounded-panel px-2 py-1.5 text-left text-text hover:bg-surface-2"
    >
      <span className="w-4 shrink-0 text-accent">
        {selected && <Icon name="check" size={14} />}
      </span>
      <span className="flex-1 truncate">{children}</span>
      {hint !== undefined && <span className="text-xs text-text-faint">{hint}</span>}
    </button>
  );
}

export function MenuLabel({
  children,
  centered = false,
}: {
  children: ReactNode;
  /**
   * Centralizado no painel de pessoas, que é uma lista informativa. Nos menus de
   * escolha o rótulo continua à esquerda, alinhado aos itens que ele encabeça.
   */
  centered?: boolean;
}) {
  return (
    <p
      className={`px-2 pb-1 pt-1.5 text-xs font-semibold uppercase tracking-wide text-text-faint ${
        centered ? 'text-center' : ''
      }`}
    >
      {children}
    </p>
  );
}
