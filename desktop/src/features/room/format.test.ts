import { describe, expect, it } from 'vitest';
import { elapsed, kbps } from './format';

describe('tempo no ar', () => {
  it('conta em minutos até a primeira hora', () => {
    expect(elapsed(0)).toBe('0:00');
    expect(elapsed(75)).toBe('1:15');
    expect(elapsed(3599)).toBe('59:59');
  });

  it('abre a casa das horas quando passa de uma', () => {
    expect(elapsed(3600)).toBe('1:00:00');
    expect(elapsed(3723)).toBe('1:02:03');
  });
});

describe('bitrate', () => {
  it('vira Mb/s acima de mil, para a pílula não crescer', () => {
    expect(kbps(850)).toBe('850 kb/s');
    expect(kbps(6200)).toBe('6.2 Mb/s');
  });
});
