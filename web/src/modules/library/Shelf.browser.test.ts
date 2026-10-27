import { describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { Game } from '../../state/api';
import Shelf from './Shelf.svelte';

const g = (appid: number, name: string, cover = ''): Game => ({ appid, name, installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 100, playtime2wkMin: 0, cover, hero: '', logo: '', achUnlocked: null, achTotal: null, updated: false, update: null, controller: null, coop: false, running: false });
const art = (w: number, h: number) => {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  c.getContext('2d')!.fillRect(0, 0, w, h);
  return c.toDataURL();
};
const list = [
  g(1, 'Portrait', art(200, 300)),
  g(2, 'Wide art', art(460, 215)),
  g(3, 'Broken art', '/no-such-cover.png'),
  g(4, 'No art'),
  g(5, 'Ori and the Blind Forest: Definitive Edition, a very long name that does not fit', art(300, 300)),
];
const sizes = () => [...document.querySelectorAll('.cover')].map((c) => c.getBoundingClientRect()).map((r) => `${Math.round(r.width)}x${Math.round(r.height)}`);

describe('Shelf', () => {
  for (const grid of [false, true]) {
    it(`every cover box is the same 2:3 size in the ${grid ? 'Show all grid' : 'row'}, whatever the art or name`, async () => {
      const shown = list.concat(grid ? [g(6, 'Six'), g(7, 'Seven')] : []);
      await render(Shelf, { props: { title: 'Backlog', games: shown, onpick: vi.fn() } });
      if (grid) await page.getByRole('button', { name: 'Show all' }).click();
      await expect.poll(() => document.querySelectorAll('.cover img').length).toBe(3); // the broken one fell back
      await expect.poll(() => new Set(sizes()).size, { timeout: 2000 }).toBe(1);
      const [w, h] = sizes()[0].split('x').map(Number);
      expect(Math.abs(h / w - 1.5)).toBeLessThan(0.02);
      if (!grid) expect(w).toBe(138);
    });
  }
});
