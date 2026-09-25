import { ApiClient, ApiError, NetworkError } from '../api/client';
import type { AuthResponse } from '../api/types/AuthResponse';
import {
  API_BASE_URL,
  CLIENT_INFO,
  GATEWAY_URL,
  UPDATE_CHECK_DELAY_MS,
  UPDATE_CHECK_INTERVAL_MS,
} from '../config';
import { listen } from '@tauri-apps/api/event';
import { notifyShareStarted, shouldNotify } from '../platform/notify';
import { GatewayClient } from '../gateway/client';
import { log } from '../log';
import { MediaSession } from '../media/session';
import { startPreviewBridge } from '../media/preview';
import { onStopRequested } from '../media/native';
import { checkForUpdate } from '../platform/updater';
import { clearRefreshToken, readRefreshToken, writeRefreshToken } from '../platform/vault';
import { useMediaStore } from '../store/media';
import { usePrivateCallStore } from '../store/privateCall';
import { useRoomStore } from '../store/room';
import { useSessionStore } from '../store/session';
import { useUpdaterStore } from '../store/updater';

/** Same wording for a wrong code and an expired one: the server does not tell them apart. */
export const PAIRING_FAILED_MESSAGE =
  'Código inválido ou expirado. Peça um novo com /tela no Discord.';

export const api = new ApiClient({
  baseUrl: API_BASE_URL,
  onSession: (auth: AuthResponse) => writeRefreshToken(auth.refresh_token),
  onSessionLost: () => {
    void signOut();
  },
});

export const gateway = new GatewayClient({
  url: GATEWAY_URL,
  client: CLIENT_INFO,
  getAccessToken: () => api.token,
  onEvent: (event) => {
    log.debug(`gateway: ${event.t}`, { d: event.d });
    if (event.t === 'READY') {
      authRetryUsed = false;
      useSessionStore.getState().signedIn(event.d.user);
      if (event.d.private_call === undefined) {
        usePrivateCallStore.getState().closed();
      } else {
        usePrivateCallStore.getState().opened(event.d.private_call);
      }
    }
    if (event.t === 'SHARE_START') {
      announceShare(event.d.user_id);
    }
    if (event.t === 'PRIVATE_CALL_JOIN') {
      usePrivateCallStore.getState().opened(event.d.call);
      void syncMediaTarget();
      return;
    }
    if (event.t === 'PRIVATE_CALL_END') {
      const active = usePrivateCallStore.getState().call;
      if (active?.id === event.d.call_id) {
        usePrivateCallStore.getState().closed();
        void syncMediaTarget();
      }
    }
    useRoomStore.getState().apply(event);
  },
  onStatus: (status) => {
    log.info(`gateway: ${status}`);
    useSessionStore.getState().setGateway(status);
  },
  onFatal: (reason) => {
    log.warn('gateway: encerrado sem retomada', { motivo: reason });
    if (reason === 'outdated_client') {
      useSessionStore.getState().setPhase('update_required');
      return;
    }
    void recoverFromAuthFailure();
  },
});

export const media = new MediaSession(api);

let started = false;
/** One token refresh per auth failure. A second in a row means the session is gone. */
let authRetryUsed = false;

export async function start(): Promise<void> {
  if (started) {
    return;
  }
  started = true;

  log.info('app: iniciando', { api: API_BASE_URL, gateway: GATEWAY_URL });

  // "Trocar de conta", na bandeja. O aplicativo não tem como descobrir qual
  // conta do Discord está aberta na máquina, então a troca é explícita: sai da
  // sessão atual — revogando o refresh token no servidor antes de apagá-lo do
  // cofre — e volta para o pareamento, onde o próximo `/tela` define quem entra.
  void listen('session://sign-out', () => {
    log.info('sessão: troca de conta pedida pela bandeja');
    void signOut();
  });

  // ADR-0030: um ouvinte só, pela vida do processo. O elemento que recebe os
  // quadros entra e sai; a assinatura não.
  startPreviewBridge();

  // A bandeja e o atalho global (Ctrl+Shift+E) pedem a parada; quem sabe se há
  // algo para parar é este lado. Vale com a janela escondida, que é o estado
  // normal do aplicativo (RF-26) e justamente quando descobrir a tela errada no
  // ar é mais caro.
  void onStopRequested(() => {
    if (!useMediaStore.getState().publishing) {
      return;
    }
    log.info('compartilhamento: parada pedida de fora da janela');
    void media.stopShare();
  });

  scheduleUpdateChecks();

  useRoomStore.subscribe((state, previous) => {
    if (state.channelId !== previous.channelId) {
      log.info('sala: o Discord mudou o canal', {
        de: previous.channelId,
        para: state.channelId,
      });
      void syncMediaTarget();
    }
  });
  usePrivateCallStore.subscribe((state, previous) => {
    if (state.call?.id !== previous.call?.id) {
      void syncMediaTarget();
    }
  });

  let stored: string | null = null;
  try {
    stored = await readRefreshToken();
  } catch {
    stored = null;
  }
  if (stored === null) {
    log.info('app: sem token no cofre, pedindo pareamento');
    useSessionStore.getState().setPhase('pairing');
    return;
  }
  log.debug('app: token encontrado no cofre, renovando sessão');

  api.seedRefreshToken(stored);
  try {
    const auth = await api.refreshSession();
    useSessionStore.getState().signedIn(auth.user);
    gateway.start();
  } catch (error) {
    log.error('app: não consegui renovar a sessão', error);
    if (error instanceof NetworkError) {
      // The server being unreachable is not a reason to make the user pair again.
      useSessionStore.getState().setPhase('authenticated');
      gateway.start();
      return;
    }
    await signOut();
  }
}

export async function pair(code: string): Promise<void> {
  const session = useSessionStore.getState();
  session.setPairingError(null);
  session.setPairing(true);
  try {
    log.info('pareamento: enviando código');
    const auth = await api.pair(code);
    log.info('pareamento: aceito', { usuario: auth.user.username });
    authRetryUsed = false;
    session.signedIn(auth.user);
    gateway.start();
  } catch (error) {
    log.error('pareamento: recusado', error);
    session.setPairingError(
      error instanceof ApiError ? PAIRING_FAILED_MESSAGE : 'Servidor indisponível. Tente de novo.',
    );
  } finally {
    session.setPairing(false);
  }
}

export async function signInWithDiscord(): Promise<void> {
  const session = useSessionStore.getState();
  session.setPairingError(null);
  session.setPairing(true);
  try {
    const attempt = await api.oauthStart();
    const popup = window.open(attempt.authorize_url, '_blank');
    if (popup === null) {
      throw new Error('O navegador bloqueou a janela de autenticação.');
    }
    const deadline = Date.now() + attempt.expires_in * 1000;
    while (Date.now() < deadline) {
      await delay(1000);
      const auth = await api.oauthComplete(attempt.attempt_id, attempt.poll_secret);
      if (auth === null) {
        continue;
      }
      popup.close();
      authRetryUsed = false;
      session.signedIn(auth.user);
      gateway.start();
      return;
    }
    throw new Error('A autorização expirou. Tente novamente.');
  } catch (error) {
    log.error('oauth: entrada falhou', error);
    session.setPairingError(
      error instanceof NetworkError
        ? 'Servidor indisponível. Tente de novo.'
        : describeLoginError(error),
    );
  } finally {
    session.setPairing(false);
  }
}

export async function createPrivateCall(): Promise<void> {
  const store = usePrivateCallStore.getState();
  store.setBusy(true);
  store.setError(null);
  try {
    const created = await api.createPrivateCall();
    store.opened(created.call, created.code);
  } catch (error) {
    store.setError(error instanceof ApiError ? error.message : 'Não foi possível criar a chamada.');
  } finally {
    usePrivateCallStore.getState().setBusy(false);
  }
}

export async function joinPrivateCall(code: string): Promise<void> {
  const store = usePrivateCallStore.getState();
  store.setBusy(true);
  store.setError(null);
  try {
    store.opened(await api.joinPrivateCall(code.trim()));
  } catch {
    store.setError('Código inválido, expirado ou já utilizado.');
  } finally {
    usePrivateCallStore.getState().setBusy(false);
  }
}

export async function endPrivateCall(): Promise<void> {
  const call = usePrivateCallStore.getState().call;
  if (call === null) {
    return;
  }
  try {
    await api.endPrivateCall(call.id);
  } catch (error) {
    usePrivateCallStore
      .getState()
      .setError(error instanceof ApiError ? error.message : 'Não foi possível encerrar a chamada.');
    return;
  }
  usePrivateCallStore.getState().closed();
  await syncMediaTarget();
}

export async function signOut(): Promise<void> {
  gateway.stop();
  await media.leave();
  await api.logout();
  useRoomStore.getState().reset();
  usePrivateCallStore.getState().closed();
  try {
    await clearRefreshToken();
  } catch {
    // Nothing to do: the token is already unusable on the server.
  }
  useSessionStore.getState().signedOut();
}

async function syncMediaTarget(): Promise<void> {
  const privateCall = usePrivateCallStore.getState().call;
  if (privateCall !== null) {
    await media.followPrivate(privateCall.id);
    return;
  }
  await media.follow(useRoomStore.getState().channelId);
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function describeLoginError(error: unknown): string {
  return error instanceof Error ? error.message : 'Não foi possível entrar com o Discord.';
}

async function recoverFromAuthFailure(): Promise<void> {
  if (authRetryUsed || !api.hasRefreshToken()) {
    await signOut();
    return;
  }
  authRetryUsed = true;
  try {
    await api.refreshSession();
    gateway.start();
  } catch {
    await signOut();
  }
}

/**
 * RF-28. First check waits for the window to settle rather than racing the
 * app's own startup; later ones repeat because the app is meant to sit in the
 * tray for days between restarts (RNF-03), and a version released on day two
 * would otherwise never be offered.
 *
 * A failed check is silent by design (`checkForUpdate` already logs it): the
 * next scheduled attempt is the retry, and a banner for "couldn't reach the
 * update server" would be noise nobody can act on.
 */
function scheduleUpdateChecks(): void {
  const run = () => {
    void checkForUpdate().then((found) => {
      if (found !== null) {
        useUpdaterStore.getState().available(found.update, found.info);
      }
    });
  };
  setTimeout(run, UPDATE_CHECK_DELAY_MS);
  setInterval(run, UPDATE_CHECK_INTERVAL_MS);
}

/**
 * RF-27. Reads the publisher's name from the room the event already updated, so
 * the notification says who rather than a bare id.
 */
function announceShare(publisherId: string): void {
  const room = useRoomStore.getState();
  if (
    !shouldNotify({
      publisherId,
      selfId: useSessionStore.getState().user?.id,
      windowFocused: document.hasFocus(),
    })
  ) {
    return;
  }
  const participant = room.participants[publisherId];
  const name = participant?.user.display_name ?? participant?.user.username ?? 'Alguém';
  void notifyShareStarted(name, room.channelName);
}
