import { describe, expect, it, vi } from 'vitest';
import { cdp, page } from 'vitest/browser';
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

  const many = Array.from({ length: 12 }, (_, i) => g(100 + i, `Game ${i}`));
  const styled = () => [...document.querySelectorAll<HTMLElement>('.slot')].filter((e) => e.getAttribute('style')).length;
  const motion = (value: string) => cdp().send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value }] });

  it('reduced motion: a fresh pop animates nothing and leaves no inline style', async () => {
    expect(matchMedia('(prefers-reduced-motion: reduce)').matches).toBe(true);
    await render(Shelf, { props: { title: 'Installed', games: many, onpick: vi.fn(), pop: { n: 1, at: performance.now() } } });
    await expect.poll(() => document.querySelectorAll('.slot').length).toBeGreaterThan(5);
    await new Promise((r) => setTimeout(r, 50));
    expect(styled()).toBe(0);
    expect(document.getAnimations()).toHaveLength(0);
  });

  it('with motion: the cards on screen pop in once, then their inline styles are gone; an old pop does nothing', async () => {
    await motion('no-preference');
    try {
      const r = await render(Shelf, { props: { title: 'Installed', games: many, onpick: vi.fn(), pop: { n: 1, at: performance.now() } } });
      await expect.poll(styled).toBeGreaterThan(5); // hidden, then tweened
      await expect.poll(styled, { timeout: 3000 }).toBe(0);
      r.unmount();
      await render(Shelf, { props: { title: 'Again', games: many, onpick: vi.fn(), pop: { n: 1, at: performance.now() - 5000 } } });
      await new Promise((res) => setTimeout(res, 50));
      expect(styled()).toBe(0);
    } finally {
      await motion('reduce');
    }
  });
});
