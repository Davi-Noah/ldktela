import { BACKOFF_CEILING_MS, backoffDelayMs, heartbeatDelayMs } from './backoff';

describe('backoffDelayMs', () => {
  const noJitter = () => 0.5;

  it('doubles from one second up to the thirty second ceiling', () => {
    const delays = [0, 1, 2, 3, 4, 5, 6].map((attempt) => backoffDelayMs(attempt, noJitter));
    expect(delays).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000]);
  });

  it('spreads each delay by thirty percent in both directions', () => {
    expect(backoffDelayMs(1, () => 0)).toBe(1400);
    expect(backoffDelayMs(1, () => 1)).toBe(2600);
  });

  it('never exceeds the ceiling plus its jitter', () => {
    expect(backoffDelayMs(50, () => 1)).toBe(Math.round(BACKOFF_CEILING_MS * 1.3));
  });
});

describe('heartbeatDelayMs', () => {
  it('adds up to ten percent so reconnected clients do not beat in lockstep', () => {
    expect(heartbeatDelayMs(30_000, () => 0)).toBe(30_000);
    expect(heartbeatDelayMs(30_000, () => 1)).toBe(33_000);
  });
});
