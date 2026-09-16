import { ApiClient, ApiError, NetworkError } from '../api/client';
import type { AuthResponse } from '../api/types/AuthResponse';
import { API_BASE_URL, CLIENT_INFO, GATEWAY_URL } from '../config';
import { GatewayClient } from '../gateway/client';
import { log } from '../log';
import { MediaSession } from '../media/session';
import { clearRefreshToken, readRefreshToken, writeRefreshToken } from '../platform/vault';
import { useRoomStore } from '../store/room';
import { useSessionStore } from '../store/session';

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

  useRoomStore.subscribe((state, previous) => {
    if (state.channelId !== previous.channelId) {
      log.info('sala: o Discord mudou o canal', {
        de: previous.channelId,
        para: state.channelId,
      });
      void media.follow(state.channelId);
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

export async function signOut(): Promise<void> {
  gateway.stop();
  await media.leave();
  await api.logout();
  useRoomStore.getState().reset();
  try {
    await clearRefreshToken();
  } catch {
    // Nothing to do: the token is already unusable on the server.
  }
  useSessionStore.getState().signedOut();
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
