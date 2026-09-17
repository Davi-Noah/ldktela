import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { describeError, log } from '../log';

/**
 * RF-28: signed `.msi` updates from the repository's GitHub Releases.
 *
 * The endpoint and public key live in `tauri.conf.json`, not here — the
 * updater plugin reads `latest.json` itself and refuses anything whose
 * signature does not match the key baked into the binary at build time. This
 * module only decides *when* to ask and *how* to report progress; it never
 * touches the signature check.
 */

export interface AvailableUpdate {
  version: string;
  notes: string | null;
}

/** `null` when the current version is already the latest. */
export async function checkForUpdate(): Promise<{ update: Update; info: AvailableUpdate } | null> {
  let update: Update | null;
  try {
    update = await check();
  } catch (error) {
    // Sem rede, ou o `latest.json` fora do ar: não vale incomodar ninguém por
    // isso. A checagem seguinte tenta de novo.
    log.warn('atualização: não consegui verificar', { erro: describeError(error) });
    return null;
  }
  if (update === null) {
    return null;
  }
  log.info('atualização: disponível', { versão: update.version });
  return { update, info: { version: update.version, notes: update.body ?? null } };
}

/**
 * Downloads, verifies and installs, then restarts into the new version.
 *
 * `onProgress` reports bytes so far out of the total — `null` when the server
 * did not send a content length, which happens and must not be treated as
 * zero progress forever.
 */
export async function installUpdate(
  update: Update,
  onProgress: (downloadedBytes: number, totalBytes: number | null) => void,
): Promise<void> {
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case 'Started':
        total = event.data.contentLength ?? null;
        onProgress(0, total);
        break;
      case 'Progress':
        downloaded += event.data.chunkLength;
        onProgress(downloaded, total);
        break;
      case 'Finished':
        onProgress(total ?? downloaded, total ?? downloaded);
        break;
    }
  });
  log.info('atualização: instalada, reiniciando');
  await relaunch();
}
