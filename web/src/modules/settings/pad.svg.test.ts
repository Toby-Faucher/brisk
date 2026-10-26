import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { BUTTON_LABELS } from './controllertest';
import * as pad from './pad.svg';

/** Bounding box of an absolute M/L/C/Q/Z path, by sampling its curves. */
function bbox(d: string) {
  const tok = d.match(/[MLCQZ]|-?\d*\.?\d+/g)!;
  const xs: number[] = [], ys: number[] = [];
  let cur = [0, 0], i = 0;
  const num = () => Number(tok[i++]);
  while (i < tok.length) {
    const c = tok[i++];
    if (c === 'Z') continue;
    const n = { M: 1, L: 1, Q: 2, C: 3 }[c]!;
    const p = [cur, ...Array.from({ length: n }, () => [num(), num()])];
    for (let t = 0; t <= 1; t += 1 / 64) {
      const b = n === 3 ? [(1 - t) ** 3, 3 * (1 - t) ** 2 * t, 3 * (1 - t) * t * t, t ** 3] : n === 2 ? [(1 - t) ** 2, 2 * (1 - t) * t, t * t] : [1 - t, t];
      const q = b.map((w, k) => [w * p[k][0], w * p[k][1]]).reduce((a, v) => [a[0] + v[0], a[1] + v[1]]);
      xs.push(q[0]); ys.push(q[1]);
    }
    cur = p[n];
  }
  return { w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys), x0: Math.min(...xs), y0: Math.min(...ys) };
}

describe('pad diagram geometry', () => {
  it('body is the official 147 x 103 mm, aspect within 1%', () => {
    const b = bbox(pad.BODY);
    expect(b.w).toBeCloseTo(147, 0);
    expect(Math.abs(b.w / b.h / (147 / 103) - 1)).toBeLessThan(0.01);
    const [vx, vy, vw, vh] = pad.VIEWBOX.split(' ').map(Number);
    expect(b.x0).toBeGreaterThan(vx);
    expect(b.x0 + b.w).toBeLessThan(vx + vw);
    expect(b.y0).toBeGreaterThan(vy);
    expect(b.y0 + b.h).toBeLessThan(vy + vh);
  });

  it('every standard button 0..16 has exactly one element', () => {
    for (let i = 0; i < BUTTON_LABELS.length; i++) expect(pad.ALL.filter((c) => c.button === i).map((c) => c.id), BUTTON_LABELS[i]).toHaveLength(1);
    expect(BUTTON_LABELS).toHaveLength(17);
  });

  it('ids are unique', () => {
    const ids = [...pad.ALL.map((c) => c.id), 'pad-body', 'pad-triggers', 'pad-dpad'];
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('PadDiagram renders every control group (so no control silently stops lighting)', () => {
    const src = readFileSync(new URL('./PadDiagram.svelte', import.meta.url), 'utf8');
    for (const g of ['STICKS', 'FACE', 'MIDDLE', 'BUMPERS', 'TRIGGERS', 'DPAD', 'EXTRA_BOXES', 'EXTRA_CIRCLES', 'PADDLES']) expect(src).toMatch(new RegExp(`#each[^}]*\\b${g}\\b`));
  });

  it('face buttons sit in an Xbox diamond (Y top, A bottom, X left, B right) at equal pitch', () => {
    const f = Object.fromEntries(pad.FACE.map((c) => [c.label, c]));
    expect(f.Y.y).toBeLessThan(f.A.y);
    expect(f.X.x).toBeLessThan(f.B.x);
    const cx = (f.X.x + f.B.x) / 2, cy = (f.Y.y + f.A.y) / 2;
    for (const c of pad.FACE) expect(Math.hypot(c.x - cx, c.y - cy)).toBeCloseTo(10.55, 0);
  });
});
