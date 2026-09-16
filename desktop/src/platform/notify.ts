import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification';
import { log } from '../log';

/**
 * RF-27: a native notification when a screen goes live in the room you are in.
 *
 * The app spends its day in the tray (RF-26), so without this the only way to
 * learn that a friend started sharing is to go looking. That is the whole point
 * of the feature — and the reason it must stay quiet when you are already
 * looking at the window.
 */

/**
 * Whether a notification is worth showing.
 *
 * Kept pure and separate from the sending, because the interesting part is the
 * silence: notifying the publisher about their own screen, or interrupting
 * someone who is already watching, is noise that teaches people to turn
 * notifications off.
 */
export function shouldNotify(options: {
  publisherId: string;
  selfId: string | undefined;
  windowFocused: boolean;
}): boolean {
  if (options.selfId !== undefined && options.publisherId === options.selfId) {
    return false;
  }
  return !options.windowFocused;
}

let permission: boolean | null = null;

async function allowed(): Promise<boolean> {
  if (permission !== null) {
    return permission;
  }
  try {
    permission = (await isPermissionGranted()) || (await requestPermission()) === 'granted';
  } catch (error) {
    log.warn('notificação: permissão indisponível', { error: String(error) });
    permission = false;
  }
  return permission;
}

export async function notifyShareStarted(publisherName: string, channelName: string | null) {
  if (!(await allowed())) {
    return;
  }
  try {
    sendNotification({
      title: channelName === null ? 'Tela compartilhada' : `Tela compartilhada em ${channelName}`,
      body: `${publisherName} começou a compartilhar.`,
    });
  } catch (error) {
    // Uma notificação que não aparece não vale derrubar nada.
    log.warn('notificação: não consegui enviar', { error: String(error) });
  }
}
