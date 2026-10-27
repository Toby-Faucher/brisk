import { describe, expect, it } from 'vitest';
import type { Preset, SessionRec } from '../../state/api';
import { gameHistory, recommendPreset, recentTrouble, fmtTotal } from './streamHistory';

const H = 3_600_000;
const s = (started: number, o: Partial<SessionRec> & { drop?: number; sent?: number | null } = {}): SessionRec => {
  const { drop, sent, ...rest } = o;
  return {
    appid: 1,
    name: 'G',
    started,
    ended: started + H,
    host: 'h',
    preset: 'crisp',
    stats: { sent: sent === undefined ? 1000 : sent, dropped: drop ?? 0 } as SessionRec['stats'],
    ...rest,
  };
};
const bad = (t: number, preset: Preset = 'crisp') => s(t, { preset, drop: 30 }); // 3%
const good = (t: number, preset: Preset = 'crisp') => s(t, { preset });

describe('gameHistory', () => {
  it('counts only closed sessions of that game', () => {
    const h = gameHistory([s(1), s(2, { ended: null }), s(3, { appid: 2 }), s(4, { appid: null })], 1);
    expect(h.count).toBe(1);
  });
  it('total and last', () => {
    const h = gameHistory([s(1000), s(5000)], 1);
    expect(h).toMatchObject({ count: 2, totalMs: 2 * H, last: 5000 });
    expect(gameHistory([], 1)).toEqual({ count: 0, totalMs: 0, last: null, usualPreset: null });
  });
  it('usualPreset ties go to the latest', () => {
    expect(
      gameHistory([s(1, { preset: 'fast' }), s(2, { preset: 'crisp' }), s(3, { preset: 'fast' }), s(4, { preset: 'crisp' })], 1)
        .usualPreset,
    ).toBe('crisp');
    expect(gameHistory([s(1, { preset: 'fast' }), s(2, { preset: 'fast' }), s(3, { preset: 'crisp' })], 1).usualPreset).toBe('fast');
  });
});

describe('recommendPreset', () => {
  it('3 of 5 sessions with 3% drops at crisp -> balanced', () => {
    expect(recommendPreset([bad(5), bad(4), bad(3), good(2), good(1)], 1)).toBe('balanced');
  });
  it('balanced -> fast', () => {
    expect(recommendPreset([bad(5, 'balanced'), bad(4, 'balanced'), bad(3, 'balanced')], 1)).toBe('fast');
  });
  it('2 of 5 -> null', () => {
    expect(recommendPreset([bad(5), bad(4), good(3), good(2), good(1)], 1)).toBeNull();
  });
  it('gate fail counts like drops', () => {
    const f = (t: number) => s(t, { stats: { sent: 1000, dropped: 0, gate: 'fail' } as SessionRec['stats'] });
    expect(recommendPreset([f(5), f(4), f(3), good(2)], 1)).toBe('balanced');
  });
  it('fast -> null', () => expect(recommendPreset([bad(3, 'fast'), bad(2, 'fast'), bad(1, 'fast')], 1)).toBeNull());
  it('custom -> null', () => expect(recommendPreset([bad(3, 'custom'), bad(2, 'custom'), bad(1, 'custom')], 1)).toBeNull());
  it('sent 0 or null stats never divide by zero', () => {
    const z = (t: number) => s(t, { sent: 0, drop: 5 });
    const n = (t: number) => s(t, { stats: null });
    expect(recommendPreset([z(5), z(4), n(3), n(2), z(1)], 1)).toBeNull();
  });
  it('older sessions beyond the newest 5 are ignored', () => {
    expect(recommendPreset([good(10), good(9), good(8), bad(3), bad(2), bad(1)], 1)).toBeNull();
  });
  it('newest session on a different preset than the bad ones -> null', () => {
    expect(recommendPreset([good(5, 'balanced'), bad(4), bad(3), bad(2)], 1)).toBeNull();
  });
  it('sorts by started itself and skips open and other-game sessions', () => {
    expect(recommendPreset([bad(1), bad(2), s(9, { ended: null }), s(8, { appid: 2 }), bad(3)], 1)).toBe('balanced');
  });
  it('gate null is not bad', () => {
    const g = (t: number) => s(t, { stats: { sent: 1000, dropped: 0, gate: null } as SessionRec['stats'] });
    expect(recommendPreset([g(3), g(2), g(1)], 1)).toBeNull();
  });
  it('bad sessions split across presets: needs 3 on the newest preset', () => {
    expect(recommendPreset([bad(5), bad(4, 'balanced'), bad(3, 'balanced'), bad(2)], 1)).toBeNull();
    expect(recentTrouble([bad(5), bad(4, 'balanced'), bad(3), good(2)], 1)).toEqual({ from: 'crisp', bad: 2, of: 4 });
  });
});

describe('fmtTotal', () => {
  it('rounds before choosing the unit', () => {
    expect(fmtTotal(59.6 * 60_000)).toBe('1 h');
    expect(fmtTotal(59.4 * 60_000)).toBe('59 min');
    expect(fmtTotal(14 * H)).toBe('14 h');
  });
});
