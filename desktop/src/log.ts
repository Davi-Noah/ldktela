/**
 * Flow and error logging for the client.
 *
 * Exists because the first real run was diagnosed almost blind: the UI showed a
 * friendly sentence, the underlying error was discarded, and the server had no
 * record of a request that never arrived. A friendly message is for the user; it
 * is not a substitute for knowing what happened.
 *
 * Everything goes to the WebView console, which `just app` shows and the
 * DevTools of a built app can open.
 */

type Level = 'debug' | 'info' | 'warn' | 'error';

/** Turns anything thrown into something readable, without losing the type. */
export function describeError(error: unknown): string {
  if (error instanceof DOMException) {
    return `${error.name}: ${error.message}`;
  }
  if (error instanceof Error) {
    const cause = error.cause;
    const causeText = cause === undefined ? '' : ` (causa: ${describeError(cause)})`;
    return `${error.name}: ${error.message}${causeText}`;
  }
  if (typeof error === 'string') {
    return error;
  }
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function emit(level: Level, event: string, detail?: Record<string, unknown>): void {
  const stamp = new Date().toISOString().slice(11, 23);
  const line = `%c${stamp} %c${event}`;
  const dim = 'color:#7a7a85';
  const strong = 'color:inherit;font-weight:600';
  if (detail === undefined) {
    console[level](line, dim, strong);
    return;
  }
  console[level](line, dim, strong, detail);
}

export const log = {
  /** Steps of a flow that are only interesting while debugging. */
  debug: (event: string, detail?: Record<string, unknown>) => emit('debug', event, detail),
  /** Milestones a person would recognise: paired, joined a room, started sharing. */
  info: (event: string, detail?: Record<string, unknown>) => emit('info', event, detail),
  /** Something recoverable went wrong. */
  warn: (event: string, detail?: Record<string, unknown>) => emit('warn', event, detail),
  /** Something the user will notice. Always carries the real error. */
  error: (event: string, error: unknown, detail?: Record<string, unknown>) =>
    emit('error', event, { ...detail, erro: describeError(error), bruto: error }),
};
