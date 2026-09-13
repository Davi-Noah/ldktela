import type { QualityChoice } from '../../store/media';
import { useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { Button } from '../../ui/Button';
import { PublisherPanel } from './PublisherPanel';

interface RoomChromeProps {
  onShare: () => void;
  onStop: () => void;
  onToggleFullscreen: () => void;
  onQuality: (choice: QualityChoice) => void;
  fullscreen: boolean;
}

const QUALITY_LABELS: { value: QualityChoice; label: string }[] = [
  { value: 'auto', label: 'Auto' },
  { value: 'high', label: '1080p' },
  { value: 'low', label: '720p' },
];

/**
 * Everything that sits over the video. It is hidden as a block by the parent, so
 * nothing here animates and nothing here stays on screen while there is a picture
 * to look at.
 */
export function RoomChrome({
  onShare,
  onStop,
  onToggleFullscreen,
  onQuality,
  fullscreen,
}: RoomChromeProps) {
  const channelName = useRoomStore((state) => state.channelName);
  const participants = useRoomStore((state) => state.participants);
  const watching = useMediaStore((state) => state.watching);
  const publishing = useMediaStore((state) => state.publishing);
  const starting = useMediaStore((state) => state.starting);
  const connection = useMediaStore((state) => state.connection);
  const quality = useMediaStore((state) => state.quality);
  const publisher = watching === null ? undefined : participants[watching];

  return (
    <>
      <header className="pointer-events-none absolute inset-x-0 top-0 flex items-center gap-2 bg-scrim px-3 py-2">
        <span className="truncate text-text">{channelName ?? 'Canal de voz'}</span>
        {publisher !== undefined && (
          <span className="truncate text-text-muted">
            · tela de {publisher.user.display_name ?? publisher.user.username}
          </span>
        )}
        {connection === 'reconnecting' && (
          <span className="ml-auto text-warning">Reconectando…</span>
        )}
      </header>

      <footer className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-3 bg-scrim px-3 py-2">
        <div className="flex items-center gap-2">
          {publishing ? (
            <Button variant="danger" onClick={onStop}>
              Parar de compartilhar
            </Button>
          ) : (
            <Button variant="primary" onClick={onShare} disabled={starting}>
              {starting ? 'Escolhendo a tela…' : 'Compartilhar tela'}
            </Button>
          )}

          {watching !== null && (
            <div role="group" aria-label="Qualidade" className="flex overflow-hidden rounded-panel">
              {QUALITY_LABELS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-pressed={quality === option.value}
                  onClick={() => {
                    onQuality(option.value);
                  }}
                  className={`px-2 py-1.5 ${
                    quality === option.value
                      ? 'bg-accent-soft text-accent'
                      : 'bg-surface-2 text-text-muted hover:bg-surface-3'
                  }`}
                >
                  {option.label}
                </button>
              ))}
            </div>
          )}

          <Button onClick={onToggleFullscreen}>
            {fullscreen ? 'Sair da tela cheia' : 'Tela cheia'}
          </Button>
        </div>

        {publishing && (
          <div className="w-64">
            <PublisherPanel />
          </div>
        )}
      </footer>
    </>
  );
}
