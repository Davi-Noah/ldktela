/**
 * Os ícones do aplicativo, em SVG inline.
 *
 * Não é uma biblioteca por decisão: são vinte formas, e qualquer pacote de
 * ícones traria centenas mais um runtime para desenhar o que um `<path>`
 * desenha (CLAUDE.md §2.11 e §5). Todos herdam `currentColor`, então a cor vem
 * do token de quem usa, nunca do ícone.
 */

export type IconName =
  | 'alert'
  | 'camera'
  | 'check'
  | 'chevron'
  | 'close'
  | 'detach'
  | 'dot'
  | 'exit-fullscreen'
  | 'eye'
  | 'eye-off'
  | 'fullscreen'
  | 'gear'
  | 'grid'
  | 'layout-side'
  | 'layout-solo'
  | 'info'
  | 'monitor'
  | 'people'
  | 'refresh'
  | 'stop'
  | 'volume'
  | 'volume-off'
  | 'window';

/** Traçado. A maioria; o desenho inteiro vive nesta tabela. */
const STROKE: Record<string, string[]> = {
  alert: ['M12 3.5 21.5 20h-19z', 'M12 10v4.5'],
  // Um corpo de câmera com a lente à frente: o mesmo peso de traço do
  // `monitor`, porque os dois aparecem lado a lado no seletor e na barra.
  camera: [
    'M4 7.5h9a1.5 1.5 0 0 1 1.5 1.5v6A1.5 1.5 0 0 1 13 16.5H4A1.5 1.5 0 0 1 2.5 15V9A1.5 1.5 0 0 1 4 7.5z',
    'M14.5 11.2l5-2.7v7l-5-2.7z',
  ],
  check: ['M5 12.5 9.5 17 19 7.5'],
  chevron: ['M6 9.5l6 6 6-6'],
  close: ['M6 6l12 12', 'M18 6 6 18'],
  detach: ['M10 5H6a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-4', 'M14 4h6v6', 'M20 4l-8 8'],
  'exit-fullscreen': [
    'M3 8h3a2 2 0 0 0 2-2V3',
    'M21 8h-3a2 2 0 0 1-2-2V3',
    'M3 16h3a2 2 0 0 1 2 2v3',
    'M21 16h-3a2 2 0 0 0-2 2v3',
  ],
  eye: ['M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z', 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z'],
  'eye-off': [
    'M10.6 6.2A10 10 0 0 1 12 6c6.5 0 10 6 10 6a17 17 0 0 1-3.2 3.9',
    'M6.2 8.1C3.6 9.7 2 12 2 12s3.5 6 10 6a10 10 0 0 0 3.6-.7',
    'M9.9 9.9a3 3 0 0 0 4.2 4.2',
    'M4 4l16 16',
  ],
  fullscreen: [
    'M8 3H5a2 2 0 0 0-2 2v3',
    'M16 3h3a2 2 0 0 1 2 2v3',
    'M8 21H5a2 2 0 0 1-2-2v-3',
    'M16 21h3a2 2 0 0 0 2-2v-3',
  ],
  // Um cubo e uma coroa de oito dentes. A versao anterior era um circulo com
  // oito raios retos saindo dele, o que e um sol — e foi lido como um sol.
  // A diferenca que faz a forma virar engrenagem e o dente ter largura e estar
  // preso a um aro, e nao ser uma linha solta.
  gear: [
    'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
    'M10 2.61L14 2.61L14.29 4.96L15.36 5.41L17.23 3.95L20.05 6.77L18.59 8.64L19.04 9.71L21.39 10L21.39 14L19.04 14.29L18.59 15.36L20.05 17.23L17.23 20.05L15.36 18.59L14.29 19.04L14 21.39L10 21.39L9.71 19.04L8.64 18.59L6.77 20.05L3.95 17.23L5.41 15.36L4.96 14.29L2.61 14L2.61 10L4.96 9.71L5.41 8.64L3.95 6.77L6.77 3.95L8.64 5.41L9.71 4.96Z',
  ],
  grid: ['M3.5 3.5h7v7h-7z', 'M13.5 3.5h7v7h-7z', 'M3.5 13.5h7v7h-7z', 'M13.5 13.5h7v7h-7z'],
  // Uma tela grande com a coluna das outras ao lado, e a mesma tela sozinha: os
  // dois arranjos do foco, desenhados como são vistos.
  'layout-side': ['M3.5 5h17v14h-17z', 'M15 5v14', 'M15 12h5.5'],
  'layout-solo': ['M3.5 5h17v14h-17z'],
  info: ['M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17z', 'M12 11v5.5'],
  monitor: [
    'M4 4.5h16a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 1-1.5 1.5H4A1.5 1.5 0 0 1 2.5 15V6A1.5 1.5 0 0 1 4 4.5z',
    'M9 20h6',
    'M12 16.5V20',
  ],
  people: [
    'M9 4.8a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4z',
    'M3 20a6 6 0 0 1 12 0',
    'M16.4 6.1a3 3 0 0 1 .6 5.9',
    'M17.3 15.4A5 5 0 0 1 21 20',
  ],
  refresh: ['M20 12a8 8 0 1 1-2.4-5.7', 'M20.5 3.5V8H16'],
  volume: ['M4 9.5h3.5L13 5v14L7.5 14.5H4z', 'M16.5 9.5a3.5 3.5 0 0 1 0 5'],
  'volume-off': ['M4 9.5h3.5L13 5v14L7.5 14.5H4z', 'M17 10l4 4', 'M21 10l-4 4'],
  window: [
    'M4 4.5h16a1.5 1.5 0 0 1 1.5 1.5v12a1.5 1.5 0 0 1-1.5 1.5H4A1.5 1.5 0 0 1 2.5 18V6A1.5 1.5 0 0 1 4 4.5z',
    'M2.5 9h19',
  ],
};

/** Preenchido. Só onde a forma cheia é o significado: parar, e "no ar". */
const FILL: Record<string, string> = {
  stop: 'M7 7h10a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1z',
  dot: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z',
};

/** Pontos: um `<path>` de 0,01 de comprimento sai redondo com `stroke-linecap`. */
const DOTS: Record<string, string> = {
  alert: 'M12 17.6v.01',
  info: 'M12 7.9v.01',
};

interface IconProps {
  name: IconName;
  size?: number;
  className?: string;
}

export function Icon({ name, size = 18, className }: IconProps) {
  const filled = FILL[name];
  const strokes = STROKE[name];
  const dot = DOTS[name];
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
      className={className}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {filled !== undefined && <path d={filled} fill="currentColor" stroke="none" />}
      {strokes?.map((d) => (
        <path key={d} d={d} />
      ))}
      {dot !== undefined && <path d={dot} strokeWidth={2.4} />}
    </svg>
  );
}
