import { describe, expect, it } from 'vitest';
import { MAX_STAGGER, planEnter } from './reveal';

const r = (top: number, left = 0) => ({ top, bottom: top + 100, left, right: left + 100 });

describe('planEnter', () => {
  it('indexes on-screen items in order, capped, and skips the rest', () => {
    const rects = [r(0), r(50), r(100), r(150), r(200), r(250), r(300), r(350), r(400), r(450), r(700), r(900), r(-200)];
    const p = planEnter(rects, 1000, 800);
    expect(p.slice(0, 10)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, MAX_STAGGER, MAX_STAGGER]);
    expect(p[10]).toBe(MAX_STAGGER); // top 700 < 800 is still on screen
    expect(p[11]).toBeNull(); // below the fold keeps its scroll reveal
    expect(p[12]).toBeNull(); // above the viewport
  });
  it('skips items beside the viewport without counting them', () => {
    expect(planEnter([r(0, 1200), r(0, 10), r(0, -300)], 1000, 800)).toEqual([null, 0, null]);
  });
});
