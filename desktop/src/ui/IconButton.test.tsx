import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { IconButton } from './IconButton';

/**
 * Guarda um defeito que não aparece em nenhum teste de comportamento e é fácil
 * de reintroduzir sem perceber.
 *
 * O `group-hover:` do Tailwind vira `.group:hover &` — um seletor de
 * descendência, que casa com **qualquer** ancestral marcado como `group`. O
 * ladrilho de tela é um `group`, então uma dica presa ao grupo anônimo abria
 * junto com as dicas de todos os outros botões do ladrilho, empilhadas em cima
 * do vídeo e recortadas ao ponto de virar ruído: "Mudo enquanto voc",
 * "Destacar e", "Janela", "Focar esta te". Foi assim que chegou o relato.
 *
 * O nome no grupo é o que prende a dica ao botão dela.
 */
describe('a dica de um botão de ícone', () => {
  it('pertence ao próprio botão, e não a qualquer ancestral marcado como grupo', () => {
    render(<IconButton icon="gear" label="Ajustes" />);
    const tip = screen.getByRole('tooltip');

    expect(tip.className).toContain('group-hover/tip:opacity-100');
    expect(
      /(^|\s)group-hover:/.test(tip.className),
      'a dica voltou a depender de um grupo anônimo',
    ).toBe(false);
  });

  it('é o rótulo acessível do botão, que só tem desenho', () => {
    render(<IconButton icon="gear" label="Ajustes" />);
    expect(screen.getByRole('button', { name: 'Ajustes' })).toBeInTheDocument();
  });
});
