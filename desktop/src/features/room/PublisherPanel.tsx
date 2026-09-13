import { useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';

/** RF-21 and RF-22, for whoever is publishing. Numbers arrive on an interval. */
export function PublisherPanel() {
  const stats = useMediaStore((state) => state.stats);
  const viewerIds = useMediaStore((state) => state.viewerIds);
  const sharingAudio = useMediaStore((state) => state.sharingAudio);
  const participants = useRoomStore((state) => state.participants);

  return (
    <div className="rounded-panel border border-border bg-surface-1 p-3">
      <p className="font-semibold text-text">Você está compartilhando</p>
      <p className="text-text-muted">{sharingAudio ? 'Com áudio do sistema' : 'Sem áudio'}</p>

      <dl className="mt-group grid grid-cols-2 gap-x-2 gap-y-row font-mono">
        <Stat label="Bitrate" value={stats === null ? '—' : `${formatKbps(stats.bitrateKbps)}`} />
        <Stat label="FPS" value={stats === null ? '—' : String(stats.fps)} />
        <Stat
          label="Resolução"
          value={stats === null || stats.width === 0 ? '—' : `${stats.width}×${stats.height}`}
        />
        <Stat label="Espectadores" value={String(viewerIds.length)} />
      </dl>

      {viewerIds.length > 0 && (
        <ul className="mt-group space-y-row">
          {viewerIds.map((id) => (
            <li key={id} className="truncate text-text-muted">
              {participants[id]?.user.display_name ?? participants[id]?.user.username ?? id}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="font-ui text-text-faint">{label}</dt>
      <dd className="text-right text-text">{value}</dd>
    </>
  );
}

function formatKbps(kbps: number): string {
  return kbps >= 1000 ? `${(kbps / 1000).toFixed(1)} Mb/s` : `${kbps} kb/s`;
}
