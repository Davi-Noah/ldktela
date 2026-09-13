import type { AuthResponse } from './types/AuthResponse';
import type { CurrentUser } from './types/CurrentUser';
import type { FieldError } from './types/FieldError';
import type { PairRequest } from './types/PairRequest';
import type { RefreshRequest } from './types/RefreshRequest';
import type { RoomState } from './types/RoomState';
import type { RoomTokenRequest } from './types/RoomTokenRequest';
import type { RoomTokenResponse } from './types/RoomTokenResponse';
import type { Snowflake } from './types/Snowflake';
import { log } from '../log';

/** An error the server produced, already in the `{ error: { … } }` shape. */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly requestId: string | null;
  readonly details: FieldError[] | null;

  constructor(
    status: number,
    code: string,
    message: string,
    requestId: string | null,
    details: FieldError[] | null,
  ) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.requestId = requestId;
    this.details = details;
  }
}

/** The request never reached the server, or the reply was not readable. */
export class NetworkError extends Error {
  constructor(cause: unknown) {
    super('não foi possível falar com o servidor');
    this.name = 'NetworkError';
    this.cause = cause;
  }
}

export interface ApiClientOptions {
  baseUrl: string;
  /** Called whenever a new token pair arrives, so the refresh token reaches the vault. */
  onSession: (auth: AuthResponse) => void | Promise<void>;
  /** Called when the refresh token is gone or rejected: the user must pair again. */
  onSessionLost: () => void;
  fetchImpl?: typeof fetch;
}

interface RequestSpec {
  method: 'GET' | 'POST';
  path: string;
  body?: unknown;
  auth: boolean;
}

const FALLBACK_MESSAGES: Record<number, string> = {
  400: 'Requisição inválida.',
  401: 'Sessão expirada.',
  403: 'Você não tem permissão para isso.',
  404: 'Não encontrado.',
  409: 'Conflito com o estado atual.',
  429: 'Muitas requisições. Tente de novo em instantes.',
};

export class ApiClient {
  private readonly options: ApiClientOptions;
  private readonly doFetch: typeof fetch;
  private accessToken: string | null = null;
  private refreshToken: string | null = null;
  /** One refresh in flight at a time, so a burst of 401s produces a single rotation. */
  private refreshing: Promise<void> | null = null;

  constructor(options: ApiClientOptions) {
    this.options = options;
    this.doFetch = options.fetchImpl ?? ((input, init) => fetch(input, init));
  }

  get token(): string | null {
    return this.accessToken;
  }

  hasRefreshToken(): boolean {
    return this.refreshToken !== null;
  }

  /** Seeds the token read from the vault at startup. */
  seedRefreshToken(token: string): void {
    this.refreshToken = token;
  }

  forget(): void {
    this.accessToken = null;
    this.refreshToken = null;
  }

  async pair(code: string): Promise<AuthResponse> {
    const body: PairRequest = { code };
    const auth = await this.request<AuthResponse>({
      method: 'POST',
      path: '/auth/pair',
      body,
      auth: false,
    });
    await this.adopt(auth);
    return auth;
  }

  async refreshSession(): Promise<AuthResponse> {
    const token = this.refreshToken;
    if (token === null) {
      this.forget();
      this.options.onSessionLost();
      throw new ApiError(401, 'UNAUTHENTICATED', 'Sessão ausente.', null, null);
    }
    const body: RefreshRequest = { refresh_token: token };
    try {
      const auth = await this.request<AuthResponse>({
        method: 'POST',
        path: '/auth/refresh',
        body,
        auth: false,
      });
      await this.adopt(auth);
      return auth;
    } catch (error) {
      // A network blip must not log the user out; a rejected token must.
      if (error instanceof ApiError && error.status === 401) {
        this.forget();
        this.options.onSessionLost();
      }
      throw error;
    }
  }

  async logout(): Promise<void> {
    const token = this.refreshToken;
    if (token !== null) {
      const body: RefreshRequest = { refresh_token: token };
      try {
        await this.request<null>({ method: 'POST', path: '/auth/logout', body, auth: true });
      } catch {
        // Logging out locally matters more than the server acknowledging it.
      }
    }
    this.forget();
  }

  currentUser(): Promise<CurrentUser> {
    return this.request<CurrentUser>({ method: 'GET', path: '/users/@me', auth: true });
  }

  room(discordChannelId: Snowflake): Promise<RoomState> {
    return this.request<RoomState>({
      method: 'GET',
      path: `/rooms/${encodeURIComponent(discordChannelId)}`,
      auth: true,
    });
  }

  roomToken(discordChannelId: Snowflake, publish: boolean): Promise<RoomTokenResponse> {
    const body: RoomTokenRequest = { publish };
    return this.request<RoomTokenResponse>({
      method: 'POST',
      path: `/rooms/${encodeURIComponent(discordChannelId)}/token`,
      body,
      auth: true,
    });
  }

  private async adopt(auth: AuthResponse): Promise<void> {
    this.accessToken = auth.access_token;
    this.refreshToken = auth.refresh_token;
    await this.options.onSession(auth);
  }

  private async request<T>(spec: RequestSpec): Promise<T> {
    const first = await this.send(spec);
    if (first.status !== 401 || !spec.auth) {
      return decode<T>(first);
    }
    // Exactly one refresh, then exactly one retry. A second 401 is a real failure.
    await this.ensureRefreshed();
    const second = await this.send(spec);
    if (second.status === 401) {
      this.forget();
      this.options.onSessionLost();
    }
    return decode<T>(second);
  }

  private ensureRefreshed(): Promise<void> {
    if (this.refreshing === null) {
      this.refreshing = this.refreshSession()
        .then(() => undefined)
        .finally(() => {
          this.refreshing = null;
        });
    }
    return this.refreshing;
  }

  private async send(spec: RequestSpec): Promise<Response> {
    const headers: Record<string, string> = { Accept: 'application/json' };
    if (spec.body !== undefined) {
      headers['Content-Type'] = 'application/json; charset=utf-8';
    }
    if (spec.auth && this.accessToken !== null) {
      headers.Authorization = `Bearer ${this.accessToken}`;
    }
    const url = `${this.options.baseUrl}${spec.path}`;
    const started = performance.now();
    try {
      const response = await this.doFetch(url, {
        method: spec.method,
        headers,
        body: spec.body === undefined ? undefined : JSON.stringify(spec.body),
      });
      const ms = Math.round(performance.now() - started);
      const line = `HTTP ${spec.method} ${spec.path} -> ${response.status}`;
      if (response.ok) {
        log.debug(line, { ms });
      } else {
        log.warn(line, { ms, requestId: response.headers.get('X-Request-Id') });
      }
      return response;
    } catch (error) {
      // O fetch falhar antes de sair da maquina e o sintoma de CORS ou de CSP,
      // e os dois sao invisiveis no log do servidor porque nada chega la.
      log.error(`HTTP ${spec.method} ${spec.path} não saiu da máquina`, error, {
        url,
        dica: 'verifique CORS no servidor e connect-src no tauri.conf.json',
      });
      throw new NetworkError(error);
    }
  }
}

async function decode<T>(response: Response): Promise<T> {
  const requestId = response.headers.get('X-Request-Id');
  const raw = await readJson(response);
  if (response.ok) {
    // 204 and empty bodies decode to null; callers that expect one declare `T = null`.
    return raw as T;
  }
  const body = readErrorBody(raw);
  throw new ApiError(
    response.status,
    body?.code ?? 'UNKNOWN',
    body?.message ?? FALLBACK_MESSAGES[response.status] ?? 'Falha inesperada no servidor.',
    body?.request_id ?? requestId,
    body?.details ?? null,
  );
}

async function readJson(response: Response): Promise<unknown> {
  let text: string;
  try {
    text = await response.text();
  } catch (error) {
    throw new NetworkError(error);
  }
  if (text.length === 0) {
    return null;
  }
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return null;
  }
}

interface DecodedError {
  code: string;
  message: string;
  request_id: string | null;
  details: FieldError[] | null;
}

function readErrorBody(raw: unknown): DecodedError | null {
  const wrapper = asRecord(raw);
  const body = asRecord(wrapper?.error);
  if (body === null) {
    return null;
  }
  const code = typeof body.code === 'string' ? body.code : null;
  const message = typeof body.message === 'string' ? body.message : null;
  if (code === null || message === null) {
    return null;
  }
  return {
    code,
    message,
    request_id: typeof body.request_id === 'string' ? body.request_id : null,
    details: readFieldErrors(body.details),
  };
}

function readFieldErrors(raw: unknown): FieldError[] | null {
  if (!Array.isArray(raw)) {
    return null;
  }
  const out: FieldError[] = [];
  for (const entry of raw) {
    const record = asRecord(entry);
    if (record !== null && typeof record.field === 'string' && typeof record.code === 'string') {
      out.push({ field: record.field, code: record.code });
    }
  }
  return out;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null;
}
