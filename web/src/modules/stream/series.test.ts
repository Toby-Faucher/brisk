import { afterEach, describe, expect, it, vi } from 'vitest';
import { downsample, SeriesUploader, UPLOAD_MS, type Point } from './series';

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
  const STARTED = 1_000_000;
  function rig() {
    vi.useFakeTimers();
    vi.setSystemTime(STARTED);
    const puts: { id: number; points: Point[] }[] = [];
    let fail = false;
    const up = new SeriesUploader((id, points) => (puts.push({ id, points }), fail ? Promise.reject(new Error('down')) : Promise.resolve()));
    // one sample per 500 ms, like StreamSession
    const run = (ms: number, id = STARTED, v = 10) => {
      for (let i = 0; i < ms / 500; i++) {
        vi.advanceTimersByTime(500);
        up.add(id, v, 60, 20);
      }
    };
    return { up, puts, run, failNext: () => (fail = true) };
  }

  it('PUTs every 30 s: the complete buckets, plus the current one again until it is complete', () => {
    const { puts, run } = rig();
    run(UPLOAD_MS); // the timer starts with the first sample (t 0.5)
    expect(puts).toEqual([]);
    run(500);
    expect(puts.length).toBe(1);
    expect(puts[0].id).toBe(STARTED);
    expect(puts[0].points.map((p) => p[0])).toEqual([0, 5, 10, 15, 20, 25, 30]);
    run(UPLOAD_MS);
    // the 30 s bucket (one sample last time) comes again, complete; nothing earlier is resent
    expect(puts[1].points.map((p) => p[0])).toEqual([30, 35, 40, 45, 50, 55, 60]);
  });

  it('flushes the partial bucket on close (disconnect, pagehide) and then sends nothing more', () => {
    const { up, puts, run } = rig();
    run(7000);
    up.close();
    expect(puts.length).toBe(1);
    expect(puts[0].points.map((p) => p[0])).toEqual([0, 5]);
    up.close();
    vi.advanceTimersByTime(UPLOAD_MS * 3);
    expect(puts.length).toBe(1);
  });

  it('a failed PUT is quiet and never throws into the sampler', async () => {
    const { up, puts, run, failNext } = rig();
    failNext();
    run(UPLOAD_MS + 500);
    await vi.advanceTimersByTimeAsync(0); // the rejection settles: no unhandled rejection
    expect(puts.length).toBe(1);
    expect(() => up.close()).not.toThrow();
  });

  it('a new session id flushes the old one first; t is clamped at 0 for a clock behind hostd', () => {
    const { up, puts, run } = rig();
    run(2000);
    run(1000, STARTED + 60_000);
    expect(puts[0].id).toBe(STARTED);
    up.close();
    expect(puts[1]).toEqual({ id: STARTED + 60_000, points: [[0, 10, 60, 20]] });
  });
});
