import { type ReactNode, useEffect, useRef, useState } from 'react';
import { media } from '../../app/runtime';
import type { RoomParticipant } from '../../api/types/RoomParticipant';
import { registerPreviewElement } from '../../media/preview';
import {
  type QualityChoice,
  SELF_ID,
  shouldSilenceOtherScreens,
  useMediaStore,
} from '../../store/media';
import { useUiStore } from '../../store/ui';
import { Icon } from '../../ui/Icon';
import { IconButton } from '../../ui/IconButton';
import { MenuItem, MenuLabel, Popover } from '../../ui/Popover';
import { Slider } from '../../ui/Slider';
import { elapsed } from './format';

interface Props {
  identity: string;
  owner: RoomParticipant | undefined;
  focused: boolean;
  /** Hidden rather than unmounted: hiding is what stops adaptiveStream pulling
      a layer, and unmounting would tear the decoder down (RF-32, ADR-0031). */
  hidden: boolean;
  detached: boolean;
  floating: boolean;
  onFocus: () => void;
  onDetach: () => void;
  onFloat: () => void;
  onFullscreen: () => void;
}

/**
 * Janela entre o clique e a ação.
 *
 * Um clique simples foca e um duplo vai para tela cheia — os dois gestos do
 * Discord. Sem esta espera, o par de cliques do duplo dispararia o foco duas
 * vezes no caminho, e a tela piscaria entre grade e foco antes de abrir.
 */
const DOUBLE_CLICK_MS = 220;

/**
 * One screen: someone else's, or — when `identity` is `SELF_ID` — our own
 * preview (ADR-0030).
 *
 * The `<video>`, `<audio>` and `<img>` are created **imperatively** and appended
 * to a container, instead of being written in JSX. Two reasons, both
 * load-bearing: React must never remount them when the layout changes between
 * grid and focus (CLAUDE.md §7), and detaching moves the very same element into
 * the picture-in-picture window (ADR-0022). An element React owns cannot be
 * moved out from under it.
 *
 * The chrome is an **overlay that fades in on hover**, not a bar below the
 * picture. A permanent strip under every tile stole height from the video and
 * ran into the room's own bar at the bottom of the window (CLAUDE.md §8).
 */
export function ScreenTile({
  identity,
  owner,
  focused,
  hidden,
  detached,
  floating,
  onFocus,
  onDetach,
  onFloat,
  onFullscreen,
}: Props) {
  const mountRef = useRef<HTMLDivElement>(null);
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isSelf = identity === SELF_ID;
  const screen = useMediaStore((state) => state.screens[identity]);
  const sharingTitle = useMediaStore((state) => state.sharingTitle);
  const silenced = useMediaStore(shouldSilenceOtherScreens);
  const setShowSelfPreview = useUiStore((state) => state.setShowSelfPreview);

  useEffect(() => {
    const mount = mountRef.current;
    if (mount === null) {
      return;
    }

    if (isSelf) {
      // Uma `<img>`, e não um `<video>`: o preview chega pronto como data URL
      // do core, quadro a quadro, e o WebView decodifica cada um fora da thread
      // principal (ADR-0030).
      const image = document.createElement('img');
      image.alt = '';
      image.draggable = false;
      image.className = 'h-full w-full bg-stage object-contain';
      mount.append(image);
      registerPreviewElement(image);
      return () => {
        registerPreviewElement(null);
        image.remove();
      };
    }

    const video = document.createElement('video');
    video.autoplay = true;
    video.playsInline = true;
    video.muted = true;
    // Liberado: é a segunda janela flutuante do produto. O Document PiP dá uma
    // só (ADR-0022), e esta é uma API distinta, com janela distinta, na mesma
    // conexão — não custa assinatura nenhuma.
    video.disablePictureInPicture = false;
    video.className = 'h-full w-full object-contain bg-stage';
    mount.append(video);

    // Screen audio rides its own element so the video can stay muted and never
    // trip the autoplay policy.
    const audio = document.createElement('audio');
    audio.autoplay = true;
    mount.append(audio);

    media.registerVideoElement(identity, video);
    media.registerAudioElement(identity, audio);
    return () => {
      media.registerVideoElement(identity, null);
      media.registerAudioElement(identity, null);
      video.remove();
      audio.remove();
    };
  }, [identity, isSelf]);

  useEffect(() => {
    return () => {
      if (clickTimer.current !== null) {
        clearTimeout(clickTimer.current);
      }
    };
  }, []);

  if (!isSelf && screen === undefined) {
    return null;
  }

  // "prévia" e não "sua tela": o preview é JPEG de 480 px a 3–12 fps
  // (ADR-0030), e quem julga a qualidade da transmissão por ele conclui que o
  // produto está quebrado. Já aconteceu.
  const name = isSelf
    ? 'Prévia da sua tela'
    : (owner?.user.display_name ?? owner?.user.username ?? 'Alguém');
  const subtitle = isSelf
    ? [sharingTitle, 'imagem reduzida, só para conferir'].filter(Boolean).join(' · ')
    : null;

  return (
    <section
      // Âncora do ladrilho: é por aqui que a grade acha a mídia para destacar,
      // sem passar por uma ref que o React controla.
      data-screen={identity}
      hidden={hidden}
      className={
        focused
          ? 'group absolute inset-0 z-10 bg-stage'
          : 'group relative min-h-0 overflow-hidden rounded-panel border border-line bg-stage'
      }
    >
      <div
        // O que a janela destacada leva embora é ESTE elemento, e não a seção
        // inteira: levando a seção, o aviso "esta tela está em outra janela" e
        // os controles iriam junto, e o aviso apareceria por cima do vídeo
        // justamente na janela onde ele está.
        data-screen-media
        ref={mountRef}
        className="h-full w-full cursor-pointer"
        onClick={() => {
          if (clickTimer.current !== null) {
            clearTimeout(clickTimer.current);
          }
          clickTimer.current = setTimeout(() => {
            clickTimer.current = null;
            onFocus();
          }, DOUBLE_CLICK_MS);
        }}
        onDoubleClick={() => {
          if (clickTimer.current !== null) {
            clearTimeout(clickTimer.current);
            clickTimer.current = null;
          }
          onFullscreen();
        }}
        role="presentation"
      />

      {detached && (
        <p className="absolute inset-0 flex items-center justify-center bg-stage text-text-muted">
          Esta tela está em outra janela.
        </p>
      )}

      {/* Crachá permanente: quem é a tela precisa ser legível sem gesto
          nenhum (RF-34). O que some no repouso são os controles. */}
      <div className="pointer-events-none absolute inset-x-0 bottom-0 flex items-end justify-between gap-2 bg-linear-to-t from-scrim to-transparent p-2">
        <div className="flex min-w-0 items-center gap-1.5 rounded-pill bg-surface-1/80 px-2 py-1">
          {isSelf ? (
            <span className="shrink-0 text-danger">
              <Icon name="dot" size={12} />
            </span>
          ) : null}
          <span className="truncate text-xs font-medium text-text">{name}</span>
          {subtitle !== null && (
            <span className="truncate text-xs text-text-muted">· {subtitle}</span>
          )}
          <Elapsed since={owner?.publishing_since} />
        </div>

        <Controls>
          {!isSelf && screen?.hasAudio === true && (
            <VolumeControl
              identity={identity}
              name={name}
              volume={screen.volume}
              silenced={silenced}
            />
          )}

          {isSelf ? (
            <IconButton
              icon="eye-off"
              label="Ocultar minha tela"
              onClick={() => {
                setShowSelfPreview(false);
              }}
            />
          ) : (
            <Popover icon="gear" label={`Qualidade da tela de ${name}`}>
              {(close) => (
                <>
                  <MenuLabel>Qualidade recebida</MenuLabel>
                  {QUALITIES.map((option) => (
                    <MenuItem
                      key={option.value}
                      selected={screen?.quality === option.value}
                      hint={option.hint}
                      onClick={() => {
                        media.setQuality(identity, option.value);
                        close();
                      }}
                    >
                      {option.label}
                    </MenuItem>
                  ))}
                </>
              )}
            </Popover>
          )}

          <IconButton
            icon="detach"
            label={detached ? 'Trazer de volta' : 'Destacar em outra janela'}
            aria-pressed={detached}
            onClick={onDetach}
          />

          {!isSelf && !detached && (
            <IconButton
              icon="pip"
              label={floating ? 'Fechar a janela flutuante' : 'Janela flutuante'}
              aria-pressed={floating}
              onClick={onFloat}
            />
          )}

          <IconButton
            icon={focused ? 'grid' : 'fullscreen'}
            label={focused ? 'Voltar para a grade' : 'Focar esta tela'}
            aria-pressed={focused}
            onClick={onFocus}
          />
        </Controls>
      </div>
    </section>
  );
}

const QUALITIES: { value: QualityChoice; label: string; hint?: string }[] = [
  { value: 'auto', label: 'Automático', hint: 'segue o tamanho' },
  { value: 'high', label: 'Alta' },
  { value: 'low', label: 'Baixa', hint: 'menos dados' },
];

/**
 * Os controles só aparecem com o ponteiro sobre o ladrilho, ou quando algo
 * dentro deles tem o foco do teclado — senão a barra seria inalcançável sem
 * mouse.
 */
function Controls({ children }: { children: ReactNode }) {
  const holdChrome = useUiStore((state) => state.holdChrome);
  const release = useRef<(() => void) | null>(null);

  useEffect(() => {
    return () => {
      release.current?.();
      release.current = null;
    };
  }, []);

  return (
    <div
      // Segura o cromo da sala junto: com o ponteiro parado sobre um controle,
      // esconder a barra levaria o cursor embora (`cursor-gone`) no meio do
      // gesto.
      onPointerEnter={() => {
        release.current?.();
        release.current = holdChrome();
      }}
      onPointerLeave={() => {
        release.current?.();
        release.current = null;
      }}
      className="chrome-fade pointer-events-auto flex shrink-0 items-center gap-1 rounded-pill bg-surface-1/80 p-0.5 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
    >
      {children}
    </div>
  );
}

interface VolumeProps {
  identity: string;
  name: string;
  volume: number;
  silenced: boolean;
}

/**
 * Volume com mudo de verdade (RF-35).
 *
 * Antes só havia o cursor: silenciar alguém era arrastar até zero, e voltar era
 * adivinhar onde estava. O mudo guarda o valor anterior.
 *
 * Quando o ADR-0028 silencia as telas alheias — porque *nós* estamos
 * transmitindo áudio — o controle diz isso aqui, no lugar do sintoma. A
 * explicação existia, mas só dentro de um parágrafo do seletor, lido minutos
 * antes.
 */
function VolumeControl({ identity, name, volume, silenced }: VolumeProps) {
  const [remembered, setRemembered] = useState(1);
  const muted = volume === 0;
  const percent = Math.round(volume * 100);

  return (
    <span className="group/vol flex items-center gap-1 pr-1">
      <IconButton
        icon={muted || silenced ? 'volume-off' : 'volume'}
        label={
          silenced
            ? 'Mudo enquanto você transmite áudio'
            : muted
              ? `Ativar o som de ${name}`
              : `Silenciar ${name}`
        }
        aria-pressed={muted}
        disabled={silenced}
        onClick={() => {
          if (muted) {
            media.setVolume(identity, remembered);
            return;
          }
          setRemembered(volume);
          media.setVolume(identity, 0);
        }}
      />
      {/* Fica escondido até o ponteiro chegar: um cursor de volume por tela,
          sempre visível, é ruído em cima do vídeo. */}
      <Slider
        value={silenced ? 0 : percent}
        disabled={silenced}
        label={`Volume da tela de ${name}`}
        onChange={(next) => {
          media.setVolume(identity, next / 100);
        }}
        className="chrome-fade w-0 opacity-0 group-hover/vol:w-20 group-hover/vol:opacity-100 group-focus-within/vol:w-20 group-focus-within/vol:opacity-100"
      />
    </span>
  );
}

/**
 * Time on air, from the server's start (RF-34). A viewer who joins twenty
 * minutes in sees twenty minutes. Ticks once a second — never per frame.
 */
function Elapsed({ since }: { since: string | undefined }) {
  const [, force] = useState(0);
  useEffect(() => {
    if (since === undefined) {
      return;
    }
    const timer = setInterval(() => {
      force((n) => n + 1);
    }, 1000);
    return () => {
      clearInterval(timer);
    };
  }, [since]);

  if (since === undefined) {
    return null;
  }
  const started = Date.parse(since);
  if (Number.isNaN(started)) {
    return null;
  }
  const seconds = Math.max(0, Math.floor((Date.now() - started) / 1000));
  return (
    <span className="shrink-0 font-mono text-xs text-text-muted" title="Tempo no ar">
      {elapsed(seconds)}
    </span>
  );
}
