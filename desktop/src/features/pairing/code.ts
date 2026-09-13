/** No 0/1/I/L/O: every character that could be misread as another is out. */
export const PAIRING_ALPHABET = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789';
export const PAIRING_CODE_LENGTH = 8;

/**
 * Uppercases and drops everything outside the alphabet. There is no lookalike
 * mapping to do: both halves of each confusable pair (0/O, 1/I/L) are excluded,
 * so a typed `O` carries no recoverable intent.
 */
export function normalizePairingCode(raw: string): string {
  let code = '';
  for (const character of raw.toUpperCase()) {
    if (code.length === PAIRING_CODE_LENGTH) {
      break;
    }
    if (PAIRING_ALPHABET.includes(character)) {
      code += character;
    }
  }
  return code;
}

export function isCompletePairingCode(code: string): boolean {
  return code.length === PAIRING_CODE_LENGTH;
}
