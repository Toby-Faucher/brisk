import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { raView, startPoll, POLL_MS, BACKOFF_MAX_MS } from './ra';
import type { Achievements } from './api';

const ok: Achievements = { available: true, unlocked: 1, total: 2, list: [], state: 'ok', source: 'retroachievements' };
const steam: Achievements = { available: true, unlocked: 1, total: 2, list: [] };

describe('raView', () => {
  it('shows ok and a stateless Steam answer as they are', () => {
    expect(raView(ok)).toEqual({ ach: ok, note: null });
    expect(raView(steam)).toEqual({ ach: steam, note: null });
  });
  it('hides off, shows the reason for unsupported, fixed wording for the rest', () => {
    expect(raView({ state: 'off' })).toEqual({ ach: null, note: null });
    expect(raView({ state: 'unsupported', reason: 'RVZ files cannot be hashed.' }).note).toBe('RVZ files cannot be hashed.');
    expect(raView({ state: 'not-found' }).note).toBe('No RetroAchievements set for this game.');
    expect(raView({ state: 'rate-limited' }).note).toBe('RetroAchievements is busy. Try again shortly.');
    expect(raView({ state: 'bad-credentials' }).note).toBe('RetroAchievements refused the login. Check the key file on the machine.');
    expect(raView({ state: 'error' }).note).toBe('Could not reach RetroAchievements.');
  });
});

describe('startPoll', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('fetches every 45 s and stops when told', async () => {
    const f = vi.fn().mockResolvedValue(ok);
    const stop = startPoll(f);
    expect(f).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(f).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(f).toHaveBeenCalledTimes(2);
    stop();
    await vi.advanceTimersByTimeAsync(POLL_MS * 3);
    expect(f).toHaveBeenCalledTimes(2);
  });

  it('never overlaps: a slow request holds the next one back', async () => {
    let done!: () => void;
    const f = vi.fn(() => new Promise<void>((r) => (done = r)));
    const stop = startPoll(f);
    await vi.advanceTimersByTimeAsync(POLL_MS * 4);
    expect(f).toHaveBeenCalledTimes(1);
    done();
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(f).toHaveBeenCalledTimes(2);
    stop();
  });

  it('a stop while a request is in flight schedules nothing', async () => {
    let done!: () => void;
    const f = vi.fn(() => new Promise<void>((r) => (done = r)));
    const stop = startPoll(f);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    stop();
    done();
    await vi.advanceTimersByTimeAsync(POLL_MS * 5);
    expect(f).toHaveBeenCalledTimes(1);
  });

  it('doubles the wait on rate-limited, caps it, resets on the next good answer', async () => {
    const f = vi.fn().mockResolvedValue({ state: 'rate-limited' });
    const stop = startPoll(f);
    await vi.advanceTimersByTimeAsync(POLL_MS); // call 1, next wait 90 s
    await vi.advanceTimersByTimeAsync(POLL_MS * 2 - 1);
    expect(f).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(f).toHaveBeenCalledTimes(2);
    for (let i = 0; i < 6; i++) await vi.advanceTimersByTimeAsync(BACKOFF_MAX_MS); // reaches the cap
    const n = f.mock.calls.length;
    await vi.advanceTimersByTimeAsync(BACKOFF_MAX_MS);
    expect(f).toHaveBeenCalledTimes(n + 1);
    f.mockResolvedValue(ok);
    await vi.advanceTimersByTimeAsync(BACKOFF_MAX_MS); // this call is good
    const m = f.mock.calls.length;
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(f).toHaveBeenCalledTimes(m + 1); // back on the 45 s beat
    stop();
  });

  it('a rejected fetch keeps polling', async () => {
    const f = vi.fn().mockRejectedValue(new Error('x'));
    const stop = startPoll(f);
    await vi.advanceTimersByTimeAsync(POLL_MS * 2);
    expect(f).toHaveBeenCalledTimes(2);
    stop();
  });
});
