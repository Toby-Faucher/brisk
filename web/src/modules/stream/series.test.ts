import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../../state/api';
import { downsample, MAX_PENDING, SeriesUploader, UPLOAD_MS, type Point } from './series';

afterEach(() => vi.useRealTimers());

describe('downsample', () => {
  it('means each 5 s bucket, t = the bucket start, the partial last bucket included', () => {
    const s: Point[] = [
      [0, 10, 60, 20],
      [0.5, 20, 58, 22],
      [4.9, 30, 62, 24],
      [5, 40, 30, 10],
      [12.5, 7, 1, 2],
    ];
    expect(downsample(s)).toEqual([
      [0, 20, 60, 22],
      [5, 40, 30, 10],
      [10, 7, 1, 2],
    ]);
    expect(downsample([])).toEqual([]);
  });
});

describe('SeriesUploader', () => {
  const ID = 1_000_000;
  /** `elapsed0`: hostd's elapsedS when this page's first sample comes. The wall clock is never read. */
  function rig(elapsed0 = 0) {
    vi.useFakeTimers();
    vi.setSystemTime(ID - 3_600_000); // a laptop clock an hour behind hostd: must not matter
    let perf = 0;
    const puts: { id: number; points: Point[] }[] = [];
    let answer: () => Promise<unknown> = () => Promise.resolve();
    const up = new SeriesUploader(
      (id, points) => (puts.push({ id, points }), answer()),
      () => perf,
    );
    // one sample per 500 ms, like StreamSession; awaits so PUT answers settle in between
    const run = async (ms: number, id = ID) => {
      for (let i = 0; i < ms / 500; i++) {
        up.add(id, elapsed0 + perf / 1000, 10, 60, 20);
        perf += 500;
        await vi.advanceTimersByTimeAsync(500);
      }
    };
    const ts = (i: number) => puts[i].points.map((p) => p[0]);
    return { up, puts, run, ts, answerWith: (f: () => Promise<unknown>) => (answer = f) };
  }

  it('PUTs every 30 s; the current bucket again until complete', async () => {
    const { puts, run, ts } = rig();
    await run(UPLOAD_MS - 500);
    expect(puts).toEqual([]);
    await run(500);
    expect(ts(0)).toEqual([0, 5, 10, 15, 20, 25]);
    await run(UPLOAD_MS);
    expect(ts(1)).toEqual([25, 30, 35, 40, 45, 50, 55]);
  });

  it('keeps unacknowledged points: a failed PUT is re-sent with the next one', async () => {
    const { run, ts, answerWith } = rig();
    answerWith(() => Promise.reject(new ApiError('down', 502)));
    await run(UPLOAD_MS);
    expect(ts(0)).toEqual([0, 5, 10, 15, 20, 25]);
    answerWith(() => Promise.resolve());
    await run(UPLOAD_MS);
    expect(ts(1)).toEqual([0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55]);
    await run(UPLOAD_MS);
    expect(ts(2)[0]).toBe(55);
  });

  it('caps what it keeps while hostd is down', async () => {
    const { puts, run, answerWith } = rig();
    answerWith(() => Promise.reject(new Error('offline')));
    await run(45 * 60_000);
    expect(puts.at(-1)!.points.length).toBeLessThanOrEqual(MAX_PENDING / 10 + 1);
    expect(puts.at(-1)!.points.at(-1)![0]).toBeGreaterThan(44 * 60);
  });

  it('flushes the partial bucket on close (disconnect, pagehide), then sends nothing more', async () => {
    const { up, puts, run, ts } = rig();
    await run(7000);
    up.close();
    await vi.advanceTimersByTimeAsync(0);
    expect(ts(0)).toEqual([0, 5]);
    up.close();
    await vi.advanceTimersByTimeAsync(UPLOAD_MS * 3);
    expect(puts.length).toBe(1);
  });

  it('a 404 or 413 from hostd ends uploads of that session quietly', async () => {
    for (const status of [404, 413]) {
      const { up, puts, run, answerWith } = rig();
      answerWith(() => Promise.reject(new ApiError('no', status)));
      await run(UPLOAD_MS);
      expect(puts.length).toBe(1);
      await run(UPLOAD_MS * 2);
      up.close();
      expect(puts.length).toBe(1);
      vi.useRealTimers();
    }
  });

  it('a reload mid-session continues from hostd elapsed and never rewrites earlier buckets', async () => {
    // The first page stored up to 45 s; the reloaded page's first sample is at hostd 47.3 s.
    const { up, puts, run, ts } = rig(47.3);
    await run(10_000);
    up.close();
    await vi.advanceTimersByTimeAsync(0);
    expect(ts(0)).toEqual([50, 55]);
    expect(puts[0].points[0][0]).toBeGreaterThanOrEqual(50);
  });

  it('a new session id flushes the old one first', async () => {
    const { up, puts, run } = rig();
    await run(2000);
    await run(7000, ID + 60_000); // its first page starts 2 s in: from the 5 s bucket on
    expect(puts[0].id).toBe(ID);
    up.close();
    expect(puts[1].id).toBe(ID + 60_000);
  });
});
