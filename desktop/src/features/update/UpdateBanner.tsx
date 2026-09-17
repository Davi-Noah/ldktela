import { useState } from 'react';
import { log } from '../../log';
import { installUpdate } from '../../platform/updater';
import { useUpdaterStore } from '../../store/updater';
import { Button } from '../../ui/Button';

/**
 * RF-28. A thin bar, not a dialog: an update is never urgent enough to block the
 * room, and the room screen exists to get out of the way of the video
 * (CLAUDE.md §8). It shows once a signed `.msi` has actually been found —
 * `checkForUpdate` already verified the signature before this ever renders.
 */
export function UpdateBanner() {
  const status = useUpdaterStore((state) => state.status);
  const update = useUpdaterStore((state) => state.update);
  const info = useUpdaterStore((state) => state.info);
  const progress = useUpdaterStore((state) => state.progress);
  const error = useUpdaterStore((state) => state.error);
  const dismissed = useUpdaterStore((state) => state.dismissed);
  const dismiss = useUpdaterStore((state) => state.dismiss);
  const [installing, setInstalling] = useState(false);

  if (status === 'idle' || update === null || info === null) {
    return null;
  }
  if (status === 'available' && dismissed) {
    return null;
  }

  const onInstall = () => {
    setInstalling(true);
    useUpdaterStore.getState().startDownload();
    installUpdate(update, (downloadedBytes, totalBytes) => {
      useUpdaterStore.getState().setProgress({ downloadedBytes, totalBytes });
    }).catch((installError: unknown) => {
      log.error('atualização: falhou', installError);
      setInstalling(false);
      useUpdaterStore.getState().fail('Não consegui instalar. Tente de novo mais tarde.');
    });
  };

  return (
    <div className="fixed inset-x-0 bottom-0 z-40 flex items-center gap-3 border-t border-line bg-surface-1 px-4 py-2 text-sm text-text">
      {status === 'downloading' ? (
        <>
          <span className="truncate">Baixando a atualização {info.version}…</span>
          <Progress
            downloadedBytes={progress?.downloadedBytes ?? 0}
            totalBytes={progress?.totalBytes ?? null}
          />
        </>
      ) : (
        <>
          <span className="truncate">Versão {info.version} disponível.</span>
          {error !== null && <span className="text-warning">{error}</span>}
          <div className="ml-auto flex items-center gap-2">
            <Button onClick={onInstall} disabled={installing}>
              Atualizar e reiniciar
            </Button>
            <button
              type="button"
              onClick={dismiss}
              className="text-text-muted hover:text-text"
              aria-label="Adiar atualização"
            >
              Agora não
            </button>
          </div>
        </>
      )}
    </div>
  );
}

function Progress({
  downloadedBytes,
  totalBytes,
}: {
  downloadedBytes: number;
  totalBytes: number | null;
}) {
  if (totalBytes === null || totalBytes === 0) {
    // O servidor nem sempre manda o tamanho total; contar bytes é melhor do
    // que travar numa barra em 0% o download inteiro.
    return <span className="font-mono text-text-muted">{formatBytes(downloadedBytes)}</span>;
  }
  const percent = Math.min(100, Math.round((downloadedBytes / totalBytes) * 100));
  return (
    <div className="flex flex-1 items-center gap-2">
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-2">
        <div className="h-full bg-accent" style={{ width: `${percent}%` }} />
      </div>
      <span className="font-mono text-text-muted">{percent}%</span>
    </div>
  );
}

function formatBytes(bytes: number): string {
  return bytes >= 1_000_000
    ? `${(bytes / 1_000_000).toFixed(1)} MB`
    : `${Math.round(bytes / 1000)} KB`;
}
