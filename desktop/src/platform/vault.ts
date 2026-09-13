import { invoke } from '@tauri-apps/api/core';

/**
 * The refresh token lives in the Windows credential vault, reachable only through
 * the Rust core (CLAUDE.md §2.8). It never touches web storage, and the access
 * token never leaves memory.
 */
export function readRefreshToken(): Promise<string | null> {
  return invoke<string | null>('vault_get_refresh_token');
}

export function writeRefreshToken(token: string): Promise<void> {
  return invoke<void>('vault_set_refresh_token', { token });
}

export function clearRefreshToken(): Promise<void> {
  return invoke<void>('vault_clear_refresh_token');
}
