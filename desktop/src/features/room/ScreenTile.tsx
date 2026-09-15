import { useEffect, useRef, useState } from 'react';
import { media } from '../../app/runtime';
import type { RoomParticipant } from '../../api/types/RoomParticipant';
import { type QualityChoice, useMediaStore } from '../../store/media';

interface Props {
  identity: string;
  owner: RoomParticipant | undefined;
  focused: boolean;
  /** Hidden rather than unmounted: hiding is what stops adaptiveStream pulling
      a layer, and unmounting would tear the decoder down (RF-32). */
  hidden: boolean;
  detached: boolean;
  onFocus: () => void;
  onDetach: () => void;
}

/**
 * One received screen.
 *
 * The `<video>` and `<audio>` are created **imperatively** and appended to a
 * container, instead of being written in JSX. Two reasons, both load-bearing:
 * React must never remount them when the layout changes between grid and focus
 * (CLAUDE.md §7), and detaching moves the very same element into the
 * picture-in-picture window (ADR-0022). An element React owns cannot be moved
 * out from under it.
 */
export function ScreenTile({
  identity,
  owner,
  focused,
  hidden,
  detached,
  onFocus,
  onDetach,
}: Props) {
  const mountRef = useRef<HTMLDivElement>(null);
  const screen = useMediaStore((state) => state.screens[identity]);

  useEffect(() => {
    const mount = mountRef.current;
    if (mount === null) {
      return;
    }
    const video = document.createElement('video');
    video.autoplay = true;
    video.playsInline = true;
    video.muted = true;
    video.disablePictureInPicture = true;
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
  }, [identity]);

  if (screen === undefined) {
    return null;
  }

  const name = owner?.user.display_name ?? owner?.user.username ?? 'Alguém';

  return (
    <section
      // O grid move ESTE elemento para a janela destacada, então ele precisa ser
      // localizável de fora sem passar por uma ref que o React controla.
      data-screen={identity}
      hidden={hidden}
      className={
        focused
          ? 'absolute inset-0 z-10 flex flex-col bg-stage'
          : 'relative flex min-h-0 flex-col overflow-hidden rounded-md border border-line bg-stage'
      }
    >
      <div
        ref={mountRef}
        className="min-h-0 flex-1 cursor-pointer"
        onDoubleClick={onFocus}
        role="presentation"
      />
      {detached && (
        <p className="absolute inset-0 flex items-center justify-center text-sm text-text-muted">
          Esta tela está em outra janela.
        </p>
      )}

      <footer className="flex items-center gap-3 border-t border-line bg-surface-1 px-3 py-2 text-xs">
        <span className="truncate font-medium text-text">{name}</span>
        <Elapsed since={owner?.publishing_since} />
        <div className="ml-auto flex items-center gap-3">
          {screen.hasAudio && (
            <label className="flex items-center gap-1" title="Volume desta tela">
              <span className="text-text-muted">Vol</span>
              <input
                type="range"
                min={0}
                max={100}
                value={Math.round(screen.volume * 100)}
                onChange={(event) => {
                  media.setVolume(identity, Number(event.target.value) / 100);
                }}
                className="w-20 accent-accent"
                aria-label={`Volume da tela de ${name}`}
              />
            </label>
          )}
          <select
            value={screen.quality}
            onChange={(event) => {
              media.setQuality(identity, event.target.value as QualityChoice);
            }}
            className="rounded border border-line bg-surface-2 px-1 py-0.5 text-text"
            aria-label={`Qualidade da tela de ${name}`}
            title="Escolhe entre as camadas que quem transmite está enviando"
          >
            <option value="auto">Automático</option>
            <option value="high">Alta</option>
            <option value="low">Baixa</option>
          </select>
          <button type="button" onClick={onFocus} className="text-text-muted hover:text-text">
            {focused ? 'Voltar' : 'Focar'}
          </button>
          <button type="button" onClick={onDetach} className="text-text-muted hover:text-text">
            {detached ? 'Trazer de volta' : 'Destacar'}
          </button>
        </div>
      </footer>
    </section>
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
    <span className="font-mono text-text-muted" title="Tempo no ar">
      {format(seconds)}
    </span>
  );
}

function format(total: number): string {
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`;
}
