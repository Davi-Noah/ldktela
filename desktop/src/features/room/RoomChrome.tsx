import { media } from '../../app/runtime';
import { type PublishPreset, PUBLISH_PRESETS, useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { Button } from '../../ui/Button';

interface RoomChromeProps {
  onShare: () => void;
  onStop: () => void;
  onToggleFullscreen: () => void;
  fullscreen: boolean;
}

/**
 * The bar over the video. Per-screen controls — volume, quality, focus, detach —
 * live on each tile, because with several screens there is no "the" screen for a
 * global control to act on (RF-31).
 */
export function RoomChrome({ onShare, onStop, onToggleFullscreen, fullscreen }: RoomChromeProps) {
  const channelName = useRoomStore((state) => state.channelName);
  const publishing = useMediaStore((state) => state.publishing);
  const starting = useMediaStore((state) => state.starting);
  const connection = useMediaStore((state) => state.connection);
  const screenCount = useMediaStore((state) => state.screenOrder.length);
  const viewers = useMediaStore((state) => state.viewerIds.length);
  const stats = useMediaStore((state) => state.stats);
  const preset = useMediaStore((state) => state.publishPreset);

  return (
    <>
      <header className="pointer-events-none absolute inset-x-0 top-0 flex items-center gap-2 bg-scrim px-3 py-2 text-sm">
        <span className="truncate text-text">{channelName ?? 'Canal de voz'}</span>
        <span className="truncate text-text-muted">
          · {screenCount === 1 ? '1 tela' : `${screenCount} telas`}
        </span>
        {connection !== 'connected' && (
          <span className="text-text-muted">· {connectionLabel(connection)}</span>
        )}
        {publishing && stats !== null && (
          <span className="ml-auto font-mono text-xs text-text-muted">
            {stats.width}×{stats.height} · {stats.fps} fps · {stats.bitrateKbps} kbps ·{' '}
            {viewers === 1 ? '1 assistindo' : `${viewers} assistindo`}
          </span>
        )}
      </header>

      <footer className="absolute inset-x-0 bottom-0 flex items-center gap-2 bg-scrim px-3 py-2">
        {publishing ? (
          <>
            <Button onClick={onStop}>Parar de compartilhar</Button>
            <PresetSwitch current={preset} />
          </>
        ) : (
          <Button onClick={onShare} disabled={starting}>
            {starting ? 'Escolhendo…' : 'Compartilhar tela'}
          </Button>
        )}
        <button
          type="button"
          onClick={onToggleFullscreen}
          className="ml-auto text-sm text-text-muted hover:text-text"
        >
          {fullscreen ? 'Sair da tela cheia' : 'Tela cheia'}
        </button>
      </footer>
    </>
  );
}

/** Changing the ladder republishes the track (RF-36), so the control says so. */
function PresetSwitch({ current }: { current: PublishPreset }) {
  return (
    <label className="flex items-center gap-1 text-xs text-text-muted">
      Enviando
      <select
        value={current}
        onChange={(event) => {
          void media.changePreset(event.target.value as PublishPreset);
        }}
        className="rounded border border-line bg-surface-2 px-1 py-0.5 text-text"
        title="Trocar reinicia a transmissão por um instante"
      >
        {PUBLISH_PRESETS.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    </label>
  );
}

function connectionLabel(state: string): string {
  switch (state) {
    case 'connecting':
      return 'conectando';
    case 'reconnecting':
      return 'reconectando';
    case 'failed':
      return 'sem conexão de mídia';
    default:
      return state;
  }
}
