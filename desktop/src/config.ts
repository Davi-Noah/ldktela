import type { ClientInfo } from './api/types/ClientInfo';

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
 * Keep `version` in sync with package.json: the gateway closes with 4010 when it
 * is below the minimum it supports, and that is what triggers the update flow.
 */
export const CLIENT_INFO: ClientInfo = { version: '0.1.0', os: 'windows' };

/** How often publisher stats are sampled. Never per frame (CLAUDE.md §7). */
export const STATS_SAMPLE_INTERVAL_MS = 2000;

/** Idle time before the chrome over a live video hides itself. */
export const CHROME_IDLE_MS = 2500;

/** RF-28. First check waits for the window to settle; later ones are spaced out
    because the app is meant to live in the tray for days at a time (RNF-03). */
export const UPDATE_CHECK_DELAY_MS = 10_000;
export const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

/**
 * Preview da própria tela (ADR-0030). Na grade ele é uma confirmação — três
 * quadros por segundo já mostram que a imagem está viva. Em foco ele vira algo
 * que se olha, e aí precisa se mexer como vídeo.
 */
export const PREVIEW_GRID_FPS = 3;
export const PREVIEW_FOCUS_FPS = 12;
