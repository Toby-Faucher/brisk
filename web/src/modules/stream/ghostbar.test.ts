import { describe, expect, it } from 'vitest';
import { edgeReveal, holdEsc } from './ghostbar';

describe('edgeReveal', () => {
  it('reveals_at_top_edge', () => {
    expect(edgeReveal(4, false, false)).toBe(true);
    expect(edgeReveal(0, false, false)).toBe(true);
    expect(edgeReveal(5, false, false)).toBe(false);
    expect(edgeReveal(300, true, true)).toBe(true); // on the bar
  });
  it('stays_while_menu_open', () => {
    expect(edgeReveal(500, true, false, true, 5000)).toBe(true);
  });
  it('hides_600ms_after_leave', () => {
    expect(edgeReveal(500, true, false, false, 599)).toBe(true);
    expect(edgeReveal(500, true, false, false, 600)).toBe(false);
    expect(edgeReveal(500, false, false, false, 100)).toBe(false); // hidden stays hidden off the edge
  });
});

describe('holdEsc', () => {
  it('hold_esc_fires_after_800ms', () => {
    const h = holdEsc();
    expect(h.press(1000)).toEqual({ progress: 0, fired: false });
    expect(h.tick(1400).progress).toBeCloseTo(0.5);
    expect(h.tick(1799).fired).toBe(false);
    expect(h.tick(1800)).toEqual({ progress: 1, fired: true });
    expect(h.release(1900)).toEqual({ progress: 0, fired: false });
  });
  it('esc_tap_does_not_fire', () => {
    const h = holdEsc();
    h.press(0);
    expect(h.release(120)).toEqual({ progress: 0, fired: false });
    expect(h.tick(2000)).toEqual({ progress: 0, fired: false });
  });
  it('auto_repeat_ignored', () => {
    const h = holdEsc();
    h.press(0);
    h.press(500); // keydown auto-repeat must not restart the hold
    h.press(700);
    expect(h.tick(800).fired).toBe(true);
    h.release(900);
    h.press(1000); // a new hold starts over
    expect(h.tick(1500).fired).toBe(false);
  });
});
