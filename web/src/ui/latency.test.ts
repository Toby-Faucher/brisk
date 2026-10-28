import { describe, expect, it } from 'vitest';
import { areaPath, edgePath, lossXs, niceMax, segments, stats, timeX, SPAN_MS, type PingSample, type Pt } from './latency';

const mk = (direct: (number | null)[], hub: (number | null)[] = direct): PingSample[] =>
  direct.map((d, i) => ({ t: i * 3000, direct: d, hub: hub[i] }));
const pts = (ys: (number | null)[]): Pt[] => ys.map((y, i) => ({ x: i * 10, y }));
const count = (d: string, ch: string) => d.split(ch).length - 1;

describe('stats', () => {
  it('empty', () => expect(stats([], 'direct')).toEqual({ now: null, min: null, avg: null, max: null, lossPct: 0 }));
  it('single', () => expect(stats(mk([12]), 'direct')).toEqual({ now: 12, min: 12, avg: 12, max: 12, lossPct: 0 }));
  it('plain run', () => expect(stats(mk([10, 20, 30]), 'direct')).toEqual({ now: 30, min: 10, avg: 20, max: 30, lossPct: 0 }));
  it('gap in the middle is loss, not zero', () => {
    const s = stats(mk([10, null, 30, 50]), 'direct');
    expect(s).toMatchObject({ now: 50, min: 10, avg: 30, max: 50, lossPct: 25 });
  });
  it('lost at the end: now is null', () =>
    expect(stats(mk([10, 20, null]), 'direct')).toMatchObject({ now: null, avg: 15, lossPct: expect.closeTo(33.33, 1) }));
  it('lost at the start', () => expect(stats(mk([null, 20]), 'direct')).toMatchObject({ now: 20, min: 20, lossPct: 50 }));
  it('all null', () => expect(stats(mk([null, null]), 'hub')).toEqual({ now: null, min: null, avg: null, max: null, lossPct: 100 }));
  it('a spike moves max and avg only', () => expect(stats(mk([5, 5, 500, 5]), 'direct')).toMatchObject({ min: 5, max: 500, avg: 128.75 }));
  it('reads the requested layer', () => expect(stats(mk([1, 2], [9, null]), 'hub')).toMatchObject({ now: null, max: 9, lossPct: 50 }));
});

describe('niceMax', () => {
  it.each([
    [0, 10],
    [-3, 10],
    [NaN, 10],
    [1, 10],
    [10, 10],
    [10.1, 20],
    [20, 20],
    [21, 50],
    [50, 50],
    [51, 100],
    [100, 100],
    [101, 200],
    [230, 500],
    [501, 1000],
    [1000, 1000],
    [4800, 5000],
    [5001, 10000],
  ])('%s -> %s', (i, o) => expect(niceMax(i)).toBe(o));
});

describe('timeX', () => {
  it('newest at the right edge, one span back at 0', () => {
    expect(timeX(1000, 1000, 300)).toBe(300);
    expect(timeX(1000 - SPAN_MS, 1000, 300)).toBeCloseTo(0);
  });
  it('one 3 s step is a 1/59 slice of the width', () => expect(300 - timeX(0, 3000, 300)).toBeCloseTo(300 / 59));
});

describe('paths', () => {
  it('splits runs at every gap', () => {
    expect(segments(pts([1, 2, null, 3, null, null, 4, 5])).map((s) => s.length)).toEqual([2, 1, 2]);
    expect(segments(pts([null, null]))).toEqual([]);
    expect(segments([])).toEqual([]);
  });
  it('edge: one M per run, no line across a gap', () => {
    const d = edgePath(pts([1, 2, 3, null, 4, 5]));
    expect(count(d, 'M')).toBe(2);
    expect(count(d, 'C')).toBe(3);
    expect(count(d, 'L')).toBe(0);
  });
  it('area: closed per run of 2+, lone points skipped, starts and ends on the baseline', () => {
    const d = areaPath(pts([1, 2, null, 3, null, 4, 5]), 90);
    expect(count(d, 'Z')).toBe(2);
    expect(d).toContain('L10 90L0 90Z');
  });
  it('a lone point is a dot stub with no area', () => {
    expect(edgePath(pts([7]))).toBe('M0 7h0');
    expect(areaPath(pts([7]), 90)).toBe('');
  });
  it('empty and all-null give empty strings', () => {
    expect(edgePath([])).toBe('');
    expect(areaPath(pts([null, null]), 90)).toBe('');
  });
  it('monotone: a spike does not overshoot the sampled range', () => {
    const d = edgePath(pts([10, 10, 80, 10, 10]));
    const ys = [...d.matchAll(/-?\d+(?:\.\d+)? (-?\d+(?:\.\d+)?)/g)].map((m) => +m[1]);
    expect(Math.min(...ys)).toBeGreaterThanOrEqual(10);
    expect(Math.max(...ys)).toBeLessThanOrEqual(80);
  });
  it('flat data stays flat', () => expect(edgePath(pts([5, 5, 5]))).toBe('M0 5C3.33 5 6.67 5 10 5C13.33 5 16.67 5 20 5'));
  it('lossXs lists the lost samples', () => expect(lossXs(pts([1, null, 2, null]))).toEqual([10, 30]));
});
