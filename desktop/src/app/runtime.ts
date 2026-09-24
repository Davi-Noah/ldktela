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
import { chimeForShare, primeChime } from '../platform/chime';
import type { PublicationSource } from '../media/publication';
import { notifyShareStarted, shouldNotify } from '../platform/notify';
import { GatewayClient } from '../gateway/client';
import { describeError, log } from '../log';
import { MediaSession } from '../media/session';
import { startPreviewBridge } from '../media/preview';
import { onStopRequested } from '../media/native';
import { checkForUpdate } from '../platform/updater';
import {
  fetchReleaseText,
  markVersionSeen,
  readSeenVersion,
  type ReleaseText,
} from '../platform/releaseNotes';
import { notesVerdict, readNotes } from '../features/update/notes';
import { useReleaseNotesStore } from '../store/releaseNotes';
import { clearRefreshToken, readRefreshToken, writeRefreshToken } from '../platform/vault';
import { shouldSilenceOtherScreens, useMediaStore } from '../store/media';
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
    }
    if (event.t === 'SHARE_START') {
      announceShare(event.d.user_id, event.d.source);
    }
    if (event.t === 'SHARE_STOP') {
      chimeForShare(
        'stop',
        event.d.user_id,
        useSessionStore.getState().user?.id,
        shouldSilenceOtherScreens(useMediaStore.getState()),
      );
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

  primeChime();

  // ADR-0030: um ouvinte só, pela vida do processo. O elemento que recebe os
  // quadros entra e sai; a assinatura não.
  startPreviewBridge();

  // A bandeja e o atalho global (Ctrl+Shift+E) pedem a parada; quem sabe se há
  // algo para parar é este lado. Vale com a janela escondida, que é o estado
  // normal do aplicativo (RF-26) e justamente quando descobrir a tela errada no
  // ar é mais caro.
  void onStopRequested(() => {
    const state = useMediaStore.getState();
    // Para as duas fontes (ADR-0038): quem usa o atalho para tirar a própria
    // imagem do ar não espera que a câmera continue transmitindo.
    if (!state.publishing && !state.camera.publishing) {
      return;
    }
    log.info('transmissão: parada pedida de fora da janela');
    if (state.publishing) {
      void media.stopShare();
    }
    if (state.camera.publishing) {
      void media.stopCamera();
    }
  });

  scheduleUpdateChecks();

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
  // Antes do pareamento, e sem esperar: é o cofre ao abrir que diz se isto
  // foi uma atualização ou uma instalação nova (ADR-0040).
  void offerReleaseNotes(stored !== null);

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
function announceShare(publisherId: string, source: PublicationSource): void {
  const room = useRoomStore.getState();
  const selfId = useSessionStore.getState().user?.id;

  // O som toca mesmo com a janela em foco, e a notificação não: quem está com o
  // aplicativo aberto costuma estar olhando para o jogo, não para a lista de
  // ladrilhos. O aviso de sistema aí seria intrusão; o sino é a única forma de
  // saber sem desviar o olhar.
  chimeForShare('start', publisherId, selfId, shouldSilenceOtherScreens(useMediaStore.getState()));

  if (!shouldNotify({ publisherId, selfId, windowFocused: document.hasFocus() })) {
    return;
  }
  const participant = room.participants[publisherId];
  const name = participant?.user.display_name ?? participant?.user.username ?? 'Alguém';
  void notifyShareStarted(name, room.channelName, source);
}

/**
 * ADR-0040. Mostra as novidades uma vez, na primeira abertura depois de uma
 * atualização.
 *
 * Nada aqui pode atrapalhar o resto da abertura, então toda falha termina em
 * silêncio. A diferença entre as falhas é o que acontece com o registro: não
 * saber (sem rede, GitHub fora do ar) deixa a versão sem marcar, para tentar de
 * novo; não haver o que mostrar marca, para não perguntar de novo.
 */
async function offerReleaseNotes(hadSession: boolean): Promise<void> {
  const current = CLIENT_INFO.version;
  let seen: string | null;
  try {
    seen = await readSeenVersion();
  } catch (error) {
    log.warn('novidades: não consegui ler o registro', { erro: describeError(error) });
    return;
  }

  const verdict = notesVerdict({ seen, current, hadSession });
  if (verdict === 'nothing') {
    return;
  }
  if (verdict === 'record') {
    await markVersionSeen(current).catch((error: unknown) => {
      log.warn('novidades: não consegui registrar a versão', { erro: describeError(error) });
    });
    return;
  }

  let text: ReleaseText | null;
  try {
    text = await fetchReleaseText(current);
  } catch (error) {
    log.warn('novidades: GitHub indisponível, tento na próxima abertura', {
      erro: describeError(error),
    });
    return;
  }
  const notes = text === null ? null : readNotes(text.markdown);
  if (notes === null || notes.blocks.length === 0) {
    log.info('novidades: esta versão não tem texto publicado', { versão: current });
    await markVersionSeen(current).catch(() => undefined);
    return;
  }
  log.info('novidades: mostrando', { de: seen, para: current });
  useReleaseNotesStore.getState().show(current, notes);
}
