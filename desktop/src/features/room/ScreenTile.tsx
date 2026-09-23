import { type ReactNode, useEffect, useRef, useState } from 'react';
import { media } from '../../app/runtime';
import type { RoomParticipant } from '../../api/types/RoomParticipant';
import { registerPreviewElement } from '../../media/preview';
import {
  isSelfPublication,
  type QualityChoice,
  shouldSilenceOtherScreens,
  useMediaStore,
} from '../../store/media';
import { type PublicationId, sourceOfPublication } from '../../media/publication';
import { publicationSince } from '../../store/room';
import { useUiStore } from '../../store/ui';
import { Icon } from '../../ui/Icon';
import { IconButton } from '../../ui/IconButton';
import { MenuItem, MenuLabel, Popover } from '../../ui/Popover';
import { Slider } from '../../ui/Slider';
import { elapsed } from './format';

interface Props {
  id: PublicationId;
  owner: RoomParticipant | undefined;
  focused: boolean;
  /** Hidden rather than unmounted: hiding is what stops adaptiveStream pulling
      a layer, and unmounting would tear the decoder down (RF-32, ADR-0031). */
  /** Onde este ladrilho cai no layout: a grade, a tela em foco ou a coluna lateral. */
  role: 'grid' | 'main' | 'rail';
  /** Fora do documento no foco exclusivo, para o `adaptiveStream` parar de baixar. */
  hidden: boolean;
  detached: boolean;
  onFocus: () => void;
  onDetach: () => void;
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
 * One publication: someone else's screen or camera, or — when the id is ours —
 * our own local preview (ADR-0030, ADR-0038).
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
  id,
  owner,
  focused,
  role,
  hidden,
  detached,
  onFocus,
  onDetach,
  onFullscreen,
}: Props) {
  const mountRef = useRef<HTMLDivElement>(null);
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isSelf = isSelfPublication(id);
  const source = sourceOfPublication(id);
  const screen = useMediaStore((state) => state.publications[id]);
  const sharingTitle = useMediaStore((state) => state.sharingTitle);
  const cameraName = useMediaStore((state) => state.camera.deviceName);
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
      // Espelhado só aqui, no próprio preview: é o que a pessoa espera de um
      // espelho. A trilha que sai não é espelhada, ou qualquer texto na frente
      // da câmera chegaria invertido aos outros (ADR-0038).
      image.className = `h-full w-full bg-stage object-contain${
        source === 'camera' ? ' -scale-x-100' : ''
      }`;
      mount.append(image);
      registerPreviewElement(source, image);
      return () => {
        registerPreviewElement(source, null);
        image.remove();
      };
    }

    const video = document.createElement('video');
    video.autoplay = true;
    video.playsInline = true;
    video.muted = true;
    // O Picture-in-Picture do próprio WebView fica desligado (ADR-0033): a
    // janela que ele abre é do Edge, com controles do Edge que não temos como
    // estilizar nem consertar — e um deles abre `edge://settings`, que num
    // WebView2 termina em ERR_INVALID_URL.
    video.disablePictureInPicture = true;
    video.className = 'h-full w-full object-contain bg-stage';
    mount.append(video);

    // Screen audio rides its own element so the video can stay muted and never
    // trip the autoplay policy.
    const audio = document.createElement('audio');
    audio.autoplay = true;
    mount.append(audio);

    media.registerVideoElement(id, video);
    media.registerAudioElement(id, audio);
    return () => {
      media.registerVideoElement(id, null);
      media.registerAudioElement(id, null);
      video.remove();
      audio.remove();
    };
  }, [id, isSelf, source]);

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

  // "prévia" e não "sua tela": o preview é um JPEG local com relógio próprio
  // (ADR-0030), não a transmissão — quem julga uma pela outra conclui que o
  // produto está quebrado. Já aconteceu.
  const person = owner?.user.display_name ?? owner?.user.username ?? 'Alguém';
  const name = isSelf
    ? source === 'camera'
      ? 'Prévia da sua câmera'
      : 'Prévia da sua tela'
    : source === 'camera'
      ? `Câmera de ${person}`
      : person;
  const subtitle = isSelf ? (source === 'camera' ? cameraName : sharingTitle) : null;

  return (
    <section
      // Âncora do ladrilho: é por aqui que a grade acha a mídia para destacar,
      // sem passar por uma ref que o React controla.
      data-screen={id}
      hidden={hidden}
      // Posição e tamanho são do CSS, por este papel (issue #7). O ladrilho em
      // foco deixou de ser `absolute inset-0`: com as outras telas ao lado, ele
      // é uma célula da grade como as demais, só que maior.
      data-role={role}
      className={`screen-tile group relative min-h-0 overflow-hidden bg-stage ${
        role === 'main'
          ? 'rounded-panel border border-accent/40'
          : 'rounded-panel border border-line'
      }`}
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
          nenhum (RF-34). O que some no repouso são os controles.

          `pb-16` não é respiro. A pílula de controles da sala é centralizada na
          base da janela, e o ladrilho desenhava crachá e botões exatamente por
          baixo dela: com duas telas lado a lado, a pílula caía em cima do crachá
          de uma e dos controles da outra, e o texto ficava ilegível.

          `px-2 pt-2` em vez de `p-2`, para não depender da ordem em que o
          Tailwind emite `padding` e `padding-bottom` no arquivo final — quem
          perde essa corrida devolve o crachá para baixo da pílula. */}
      <div
        // `pb-16` só onde a pílula de controles passa: ela é centralizada na
        // base da janela, e sem esse respiro caía em cima do crachá e dos
        // botões. Na coluna lateral ela não passa, e o mesmo respiro deixava o
        // crachá boiando no meio do ladrilho (issue #7).
        className={`pointer-events-none absolute inset-x-0 bottom-0 flex items-end bg-linear-to-t from-scrim to-transparent px-2 pt-2 ${
          role === 'rail' ? 'pb-2' : 'pb-16'
        }`}
      >
        <div
          // O crachá e os controles disputam a mesma faixa, e num ladrilho
          // estreito o nome perdia: virava "8:" espremido contra os botões.
          // Some enquanto os controles estão à mostra e volta quando eles saem —
          // os dois nunca precisam ser lidos ao mesmo tempo.
          className="tile-badge chrome-fade flex min-w-0 items-center gap-1.5 rounded-pill bg-surface-1/80 px-2 py-1 group-hover:opacity-0 group-focus-within:opacity-0"
        >
          {isSelf ? (
            <span className="shrink-0 text-danger">
              <Icon name="dot" size={12} />
            </span>
          ) : null}
          <span className="truncate text-xs font-medium text-text">{name}</span>
          {subtitle !== null && (
            <span className="truncate text-xs text-text-muted">· {subtitle}</span>
          )}
          <Elapsed since={publicationSince(owner, source) ?? undefined} />
        </div>

        {/* Fora do fluxo, e não ao lado do crachá: invisíveis, os controles
            continuavam ocupando a linha, e num ladrilho estreito espremiam o
            nome até sobrar "b..". Como o crachá some justamente quando eles
            aparecem, os dois podem ocupar o mesmo lugar. */}
        <div className={`absolute right-2 ${role === 'rail' ? 'bottom-2' : 'bottom-16'}`}>
          <Controls>
            {!isSelf && screen?.hasAudio === true && (
              <VolumeControl id={id} name={name} volume={screen.volume} silenced={silenced} />
            )}

            {isSelf ? (
              <IconButton
                tipAlign="end"
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
                          media.setQuality(id, option.value);
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

            {!isSelf && (
              <IconButton
                tipAlign="end"
                icon="eye-off"
                label={`Sair da tela de ${name}`}
                onClick={() => {
                  media.setPublicationSubscribed(id, false);
                }}
              />
            )}

            <IconButton
              tipAlign="end"
              icon="detach"
              label={detached ? 'Trazer de volta' : 'Destacar em outra janela'}
              aria-pressed={detached}
              onClick={onDetach}
            />

            <IconButton
              tipAlign="end"
              icon={focused ? 'grid' : 'fullscreen'}
              label={focused ? 'Voltar para a grade' : 'Focar esta tela'}
              aria-pressed={focused}
              onClick={onFocus}
            />
          </Controls>
        </div>
      </div>
    </section>
  );
}

// O que "baixa" faz mudou com o ADR-0032: a escada do VP9 é temporal, então ela
// corta quadros e não pixels. A dica diz isso, porque quem escolhe "baixa"
// esperando uma imagem menor e recebe a mesma imagem travada acha que quebrou.
const QUALITIES: { value: QualityChoice; label: string; hint?: string }[] = [
  { value: 'auto', label: 'Automático', hint: 'segue o tamanho' },
  { value: 'high', label: 'Alta' },
  { value: 'low', label: 'Baixa', hint: 'menos quadros' },
];

/**
 * Os controles só aparecem com o ponteiro sobre o ladrilho, ou quando algo
 * dentro deles tem o foco do teclado — senão a barra seria inalcançável sem
 * mouse.
 */
function Controls({ children }: { children: ReactNode }) {
  return (
    <div
      // Segura o cromo da sala junto: com o ponteiro parado sobre um controle,
      // esconder a barra levaria o cursor embora (`cursor-gone`) no meio do
      // gesto. Quem lê este atributo é o `:hover` em `RoomScreen`.
      data-chrome-hold
      className="chrome-fade pointer-events-auto flex shrink-0 items-center gap-1 rounded-pill bg-surface-1/80 p-0.5 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
    >
      {children}
    </div>
  );
}

interface VolumeProps {
  id: PublicationId;
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
function VolumeControl({ id, name, volume, silenced }: VolumeProps) {
  const [remembered, setRemembered] = useState(1);
  const muted = volume === 0;
  const percent = Math.round(volume * 100);

  return (
    <span className="group/vol flex items-center gap-1 pr-1">
      <IconButton
        tipAlign="end"
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
            media.setVolume(id, remembered);
            return;
          }
          setRemembered(volume);
          media.setVolume(id, 0);
        }}
      />
      {/* Fica escondido até o ponteiro chegar: um cursor de volume por tela,
          sempre visível, é ruído em cima do vídeo. */}
      <Slider
        value={silenced ? 0 : percent}
        disabled={silenced}
        label={`Volume da tela de ${name}`}
        onChange={(next) => {
          media.setVolume(id, next / 100);
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
    <span className="tile-elapsed shrink-0 font-mono text-xs text-text-muted" title="Tempo no ar">
      {elapsed(seconds)}
    </span>
  );
}
