import fs from 'node:fs';
import { describe, expect, it } from 'vitest';
// @ts-ignore plain .mjs script, no types
import { FILE, render, springs, t90 } from '../../scripts/gen-spring-tokens.mjs';

describe('generated spring tokens', () => {
  it('tokens.css matches the generator (run node web/scripts/gen-spring-tokens.mjs if this fails)', () => {
    const css = fs.readFileSync(FILE, 'utf8');
    expect(render(css)).toBe(css);
  });
  it('hover springs are quick: 90 % within 250 ms, nothing longer than 900 ms, soft capped at 450 ms', () => {
    const s = Object.fromEntries(springs().map((x: { name: string; t90: number; dur: number; peak: number }) => [x.name, x]));
    expect(s.soft.t90).toBeLessThanOrEqual(250);
    expect(s.soft.dur).toBeLessThanOrEqual(450);
    expect(s.snappy.t90).toBeLessThanOrEqual(250);
    for (const x of Object.values(s)) expect(x.dur <= 900 && x.peak < 1.03).toBe(true);
  });
  it('t90 interpolates between samples', () => {
    expect(t90([0, 0.5, 1], 200)).toBe(180);
  });
});
