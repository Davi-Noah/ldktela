export const BACKOFF_BASE_MS = 1000;
export const BACKOFF_CEILING_MS = 30_000;
export const BACKOFF_JITTER_RATIO = 0.3;
/** A session that lasted this long resets the attempt counter (websocket.md §3.6). */
export const SESSION_STABLE_MS = 60_000;
export const HEARTBEAT_JITTER_RATIO = 0.1;

/** 1s, 2s, 4s, 8s, 16s, then a 30s ceiling, each with ±30% jitter. */
export function backoffDelayMs(attempt: number, random: () => number = Math.random): number {
  const steps = Math.max(0, Math.min(attempt, 16));
  const base = Math.min(BACKOFF_BASE_MS * 2 ** steps, BACKOFF_CEILING_MS);
  const jitter = (random() * 2 - 1) * BACKOFF_JITTER_RATIO;
  return Math.max(0, Math.round(base * (1 + jitter)));
}

/** Heartbeats are spread by up to 10% so reconnecting clients do not sync into a herd. */
export function heartbeatDelayMs(intervalMs: number, random: () => number = Math.random): number {
  return Math.round(intervalMs * (1 + random() * HEARTBEAT_JITTER_RATIO));
}
