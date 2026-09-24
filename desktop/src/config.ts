import type { ClientInfo } from './api/types/ClientInfo';
import { version as packageVersion } from '../package.json';

const DEFAULT_ORIGIN = 'http://127.0.0.1:8080';

function serverOrigin(): string {
  const configured: unknown = import.meta.env.VITE_SERVER_ORIGIN;
  if (typeof configured === 'string' && configured.length > 0) {
    return configured.replace(/\/+$/, '');
  }
  return DEFAULT_ORIGIN;
}

const ORIGIN = serverOrigin();

export const API_BASE_URL = `${ORIGIN}/api/v1`;

/** The gateway lives outside `/api/v1`. */
export const GATEWAY_URL = `${ORIGIN.replace(/^http/, 'ws')}/gateway?v=1`;

/**
 * Sai do `package.json`, e não de uma constante escrita à mão.
 *
 * Escrita à mão ela derivou: ficou em `0.1.0` por duas versões enquanto o
 * aplicativo já era 1.1.0. O gateway fecha com 4010 quem estiver abaixo do
 * mínimo, e esse fechamento é o que dispara a atualização — uma versão mentida
 * aqui tranca todo mundo para fora, ou deixa entrar quem não deveria.
 */
export const CLIENT_INFO: ClientInfo = { version: packageVersion, os: 'windows' };

/** How often publisher stats are sampled. Never per frame (CLAUDE.md §7). */
export const STATS_SAMPLE_INTERVAL_MS = 2000;

/** Idle time before the chrome over a live video hides itself. */
export const CHROME_IDLE_MS = 2500;

/** RF-28. First check waits for the window to settle; later ones are spaced out
    because the app is meant to live in the tray for days at a time (RNF-03). */
export const UPDATE_CHECK_DELAY_MS = 10_000;
export const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

/**
 * Preview da própria tela (ADR-0030). Na grade, seis quadros por segundo — três
 * mostravam que a imagem estava viva e também que parecia travada. Em foco ele
 * vira algo que se olha, e aí precisa se mexer como vídeo.
 */
export const PREVIEW_GRID_FPS = 6;
export const PREVIEW_FOCUS_FPS = 12;

/**
 * ADR-0040. Onde ficam os releases cujo texto vira o modal de novidades.
 *
 * O mesmo repositório do endpoint do atualizador em `tauri.conf.json` — um
 * teste garante que os dois não se separem, porque buscar as notas num
 * repositório e as atualizações em outro mostraria a novidade errada.
 */
export const RELEASES_API = 'https://api.github.com/repos/gbrlevi/ldktela/releases';

/** Quanto esperar pelo GitHub antes de desistir até a próxima abertura. */
export const RELEASE_NOTES_TIMEOUT_MS = 10_000;
