import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { navigate } from 'astro:transitions/client';
import type { Game } from '../../state/api';
import GameCard from './GameCard.svelte';
import { notes } from '../../state/notes';
import { quitting } from '../../shell/QuitDialog.svelte';

const game: Game = {
  appid: 400,
  name: 'Portal',
  installed: false,
  sizeBytes: 5e9,
  lastPlayed: null,
  playtimeMin: 0,
  playtime2wkMin: 0,
  cover: '',
  hero: '',
  logo: '',
  achUnlocked: null,
  achTotal: null,
  updated: false,
  update: null,
  controller: null,
  coop: false,
  running: false,
};

describe('GameCard', () => {
  it('clicking a not-installed card calls onpick and never installs', async () => {
    const onpick = vi.fn();
    await render(GameCard, { props: { game, onpick } });
    const card = page.getByRole('button', { name: /^Portal, not installed/ });
    await expect.element(card).toHaveAttribute('aria-haspopup', 'dialog');
    await userEvent.click(card);
    expect(onpick).toHaveBeenCalledExactlyOnceWith(game);
    expect(page.getByRole('button', { name: /Install/ }).elements()).toHaveLength(0);
  });

  it('clicking an installed card calls onpick without starting the stream', async () => {
    const onpick = vi.fn();
    const g = { ...game, installed: true };
    vi.mocked(navigate).mockClear();
    await render(GameCard, { props: { game: g, onpick } });
    await expect.element(page.getByText('Details')).toBeInTheDocument();
    await userEvent.click(page.getByRole('button', { name: /^Portal, ready to play/ }));
    expect(onpick).toHaveBeenCalledExactlyOnceWith(g);
    expect(navigate).not.toHaveBeenCalled();
  });

  it('shows a thin progress bar when achTotal > 0', async () => {
    await render(GameCard, { props: { game: { ...game, achUnlocked: 3, achTotal: 5 }, onpick: vi.fn() } });
    await expect.element(page.getByRole('button', { name: /3 of 5 achievements/ })).toBeInTheDocument();
    expect(page.getByRole('progressbar').elements()).toHaveLength(0);
    expect(document.querySelector('.ach')?.getAttribute('aria-hidden')).toBe('true');
  });

  it('shows no progress bar when achTotal is null or 0', async () => {
    const { rerender } = await render(GameCard, { props: { game, onpick: vi.fn() } });
    expect(document.querySelector('.ach')).toBeNull();
    await rerender({ game: { ...game, achUnlocked: 0, achTotal: 0 }, onpick: vi.fn() });
    expect(document.querySelector('.ach')).toBeNull();
  });

  it('shows an Updated chip and names it in the label', async () => {
    await render(GameCard, { props: { game: { ...game, installed: true, updated: true }, onpick: vi.fn() } });
    await expect.element(page.getByRole('button', { name: /updated since you played/ })).toBeInTheDocument();
    expect(page.getByRole('button', { name: /updated since you played/ }).element().textContent).toContain('Updated');
  });

  it('names Updated once: no auto-updated tag in the label or the tag chip', async () => {
    await render(GameCard, { props: { game: { ...game, installed: true, updated: true }, onpick: vi.fn() } });
    const label = page
      .getByRole('button', { name: /updated since you played/ })
      .element()
      .getAttribute('aria-label')!;
    expect(label.match(/updated/gi)).toHaveLength(1);
    expect(label).not.toMatch(/tags:.*Updated/);
    expect(document.querySelector('.tchip')?.querySelectorAll('.tc').length ?? 0).toBe(1); // Backlog only
  });

  it('shows the Steam update state as a chip and in the label', async () => {
    await render(GameCard, { props: { game: { ...game, installed: true, update: 'running' }, onpick: vi.fn() } });
    const b = page.getByRole('button', { name: /updating…/ });
    await expect.element(b).toBeInTheDocument();
    expect(b.element().textContent).toContain('Updating…');
  });

  it('shows no Updated chip otherwise', async () => {
    await render(GameCard, { props: { game, onpick: vi.fn() } });
    expect(page.getByText('Updated').elements()).toHaveLength(0);
  });

  it('shows up to 3 tag symbols, +n and a note mark, all named in the label', async () => {
    const t = (name: string) => ({ name, color: 'blue' as const, symbol: 'star', note: '' });
    notes.set({
      tags: { a: t('Co-op night'), b: t('Rainy day'), c: t('Short'), d: t('Cozy') },
      games: { '400': { note: 'hi', tags: ['a', 'b', 'c', 'd'] } },
    });
    await render(GameCard, { props: { game, onpick: vi.fn() } });
    await expect
      .element(page.getByRole('button', { name: /, tags: Co-op night, Rainy day, Short, Cozy, Backlog, note$/ }))
      .toBeInTheDocument();
    const chip = document.querySelector('.tchip')!;
    expect(chip.getAttribute('aria-hidden')).toBe('true');
    expect(chip.querySelectorAll('.tc')).toHaveLength(3);
    expect(chip.textContent).toContain('+2');
    expect(chip.querySelector('.nm')).not.toBeNull();
    notes.set({ tags: {}, games: {} });
  });
});

describe('GameCard lift', () => {
  // The harness runs with reduced motion, so the lift itself is off here (and must stay off for Quit too).
  it('Quit sits inside the element that lifts, which is off under reduced motion', async () => {
    await render(GameCard, { props: { game: { ...game, installed: true, running: true }, onpick: vi.fn() } });
    const card = page.getByRole('button', { name: /^Portal, running/ }).element() as HTMLElement;
    const quit = page.getByRole('button', { name: 'Quit Portal' }).element() as HTMLElement;
    const wrap = quit.closest('.wrap') as HTMLElement;
    expect(wrap.contains(card)).toBe(true);
    const rules = [...document.styleSheets].flatMap((s) => [...s.cssRules]);
    const lift = (r: CSSRule): r is CSSStyleRule =>
      r instanceof CSSStyleRule && r.selectorText.includes('.wrap') && r.selectorText.includes(':hover');
    expect(rules.some((r) => lift(r) && r.style.transform.includes('scale'))).toBe(true);
    await userEvent.hover(quit);
    expect(getComputedStyle(wrap).transform).toBe('none');
    await userEvent.tab();
    expect(getComputedStyle(wrap).transform).toBe('none');
  });
});

describe('GameCard quit spinner', () => {
  it('shows a spinner while the quit is pending and keeps the button size', async () => {
    await render(GameCard, { props: { game: { ...game, installed: true, running: true }, onpick: vi.fn() } });
    const btn = page.getByRole('button', { name: 'Quit Portal' }).element() as HTMLElement;
    const before = btn.getBoundingClientRect();
    expect(btn.querySelector('[data-testid=quit-spinner]')).toBeNull();
    quitting.set(new Set([400]));
    await expect.poll(() => btn.querySelector('[data-testid=quit-spinner]')).not.toBeNull();
    expect(btn.getAttribute('aria-busy')).toBe('true');
    expect(btn.getAttribute('aria-label')).toBe('Quitting Portal');
    const after = btn.getBoundingClientRect();
    expect([after.width, after.height]).toEqual([before.width, before.height]);
    quitting.set(new Set());
  });
});
