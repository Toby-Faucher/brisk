import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, type Game } from '../../state/api';
import { games, gamesFailed } from '../../state/games';
import { notes, notesLoaded } from '../../state/notes';
import Library from './Library.svelte';
import Toasts from '../../shell/Toasts.svelte';

const g = (appid: number, name: string, o: Partial<Game> = {}): Game => ({ appid, name, installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 100, playtime2wkMin: 0, cover: '', hero: '', logo: '', achUnlocked: null, achTotal: null, updated: false, update: null, controller: null, coop: false, running: false, ...o });
const list = [g(1, 'Alpha', { playtimeMin: 300, achUnlocked: 9, achTotal: 10 }), g(2, 'Bravo', { playtimeMin: 200 }), g(3, 'Charlie', { installed: false, playtimeMin: 5, coop: true })];

afterEach(() => {
  vi.restoreAllMocks();
  history.replaceState(null, '', location.pathname);
});
beforeEach(() => {
  notes.set({ tags: {}, games: {} });
  notesLoaded.set(true);
  games.set({ games: list, ownedAvailable: true });
  gamesFailed.set(false);
  vi.spyOn(api, 'games').mockResolvedValue({ games: list, ownedAvailable: true });
  vi.spyOn(api, 'notes').mockResolvedValue({ tags: {}, games: {} });
  vi.spyOn(api, 'prefs').mockResolvedValue({ prefs: {} });
  vi.spyOn(api, 'gameInfo').mockResolvedValue({ available: false, description: '', genres: [], controller: null, singleplayer: false, coop: false, remotePlayTogether: false, release: null, metacritic: null, screenshots: [] });
  vi.spyOn(api, 'gameAchievements').mockResolvedValue({ available: false, unlocked: 0, total: 0, list: [] });
  vi.spyOn(api, 'gameNews').mockResolvedValue({ items: [] });
  vi.spyOn(api, 'gameLocal').mockResolvedValue({ screenshots: [], saves: null });
});

describe('Library', () => {
  it('Surprise me opens the panel of a game that matches the active filter', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0.99);
    await render(Library);
    await userEvent.click(page.getByRole('button', { name: 'Installed', exact: true }));
    await userEvent.click(page.getByRole('button', { name: 'Surprise me' }));
    // Installed shows Alpha and Bravo only; the last one is picked, never Charlie
    await expect.element(page.getByRole('dialog').getByText('Bravo', { exact: true })).toBeInTheDocument();
  });

  it('Surprise me says why when nothing matches', async () => {
    await render(Library);
    await userEvent.fill(page.getByRole('searchbox', { name: 'Search games' }), 'zzz');
    const b = page.getByRole('button', { name: 'Surprise me' });
    await expect.element(b).toHaveAttribute('aria-disabled', 'true');
    await expect.element(b).toHaveAttribute('title', 'No game matches the current filters');
  });

  it('opens with the tag filter from ?tag=', async () => {
    history.replaceState(null, '', '?tag=auto-coop');
    await render(Library);
    await expect.element(page.getByRole('button', { name: /^Co-op 1$/ })).toHaveAttribute('aria-pressed', 'true');
    expect(page.getByRole('button', { name: /^Alpha/ }).elements()).toHaveLength(0);
  });

  it('opens with one of your tags from ?tag=', async () => {
    const mine = { tags: { mine: { name: 'Mine', color: 'red' as const, symbol: 'star', note: '' } }, games: { 2: { note: '', tags: ['mine'] } } };
    notes.set(mine);
    vi.spyOn(api, 'notes').mockResolvedValue(mine);
    history.replaceState(null, '', '?tag=mine');
    await render(Library);
    await expect.element(page.getByRole('button', { name: 'Mine', exact: true })).toHaveAttribute('aria-pressed', 'true');
    expect(page.getByRole('button', { name: /^Alpha/ }).elements()).toHaveLength(0);
  });

  it('ignores an unknown ?tag=', async () => {
    history.replaceState(null, '', '?tag=nope');
    await render(Library);
    await expect.element(page.getByRole('button', { name: /^Co-op 1$/ })).toBeInTheDocument();
    for (const n of ['Alpha', 'Bravo', 'Charlie']) expect(page.getByRole('button', { name: new RegExp(`^${n}`) }).elements().length).toBeGreaterThan(0);
    await expect.element(page.getByRole('button', { name: /^Co-op 1$/ })).toHaveAttribute('aria-pressed', 'false');
  });

  it('keeps ?tag= in step with the chips (other params, hash and state stay)', async () => {
    history.replaceState({ k: 1 }, '', '?tag=auto-coop&x=1#h');
    await render(Library);
    const len = history.length;
    await userEvent.click(page.getByRole('button', { name: /^Co-op 1$/ }));
    expect(location.search).toBe('?x=1');
    await userEvent.click(page.getByRole('button', { name: /^Co-op 1$/ }));
    expect(location.search).toBe('?x=1&tag=auto-coop');
    expect(location.hash).toBe('#h');
    expect(history.state).toEqual({ k: 1 });
    expect(history.length).toBe(len);
  });

  it('lists smart tags that match with counts, and filters by one', async () => {
    await render(Library);
    await expect.element(page.getByRole('button', { name: /^Co-op 1$/ })).toBeInTheDocument();
    expect(page.getByRole('button', { name: /^Controller/ }).elements()).toHaveLength(0);
    await userEvent.click(page.getByRole('button', { name: /^Co-op 1$/ }));
    await expect.element(page.getByRole('button', { name: /^Charlie/ })).toBeInTheDocument();
    expect(page.getByRole('button', { name: /^Alpha/ }).elements()).toHaveLength(0);
  });

  it('orders the shelves: fortnight, Installed, Almost done, Backlog, Not installed', async () => {
    const two = list.map((x) => (x.appid === 2 ? { ...x, playtime2wkMin: 30 } : x));
    games.set({ games: two, ownedAvailable: true });
    vi.spyOn(api, 'games').mockResolvedValue({ games: two, ownedAvailable: true });
    await render(Library);
    await expect.element(page.getByRole('heading', { name: /^Backlog \d+$/ })).toBeInTheDocument();
    const order = [...document.querySelectorAll('.shelf h4 > span:first-child')].map((h) => h.textContent!.replace(/\s*\d+$/, '').trim());
    expect(order).toEqual(['Played this fortnight', 'Installed', 'Almost done', 'Backlog', 'Not installed']);
  });

  it('the sort dropdown reorders the shelves', async () => {
    const two = [g(7, 'Zed', { playtimeMin: 900 }), g(8, 'Able', { playtimeMin: 800 })];
    games.set({ games: two, ownedAvailable: true });
    vi.spyOn(api, 'games').mockResolvedValue({ games: two, ownedAvailable: true });
    await render(Library);
    const names = () => [...document.querySelectorAll('.shelf .name')].map((e) => e.textContent);
    await expect.poll(names).toEqual(['Zed', 'Able']);
    await userEvent.click(page.getByRole('combobox', { name: 'Sort games' }));
    await userEvent.click(page.getByRole('option', { name: 'A-Z' }));
    await expect.poll(names).toEqual(['Able', 'Zed']);
    await expect.element(page.getByRole('combobox', { name: 'Sort games' })).toHaveTextContent('A-Z');
  });

  it('Backlog and Almost done shelves show only with no search, filter or tag', async () => {
    const shelf = (n: string) => page.getByRole('heading', { name: new RegExp(`^${n} \\d+$`) }).elements().length;
    await render(Library);
    await expect.element(page.getByRole('heading', { name: /^Backlog \d+$/ })).toBeInTheDocument();
    expect(shelf('Almost done')).toBe(1);
    await userEvent.fill(page.getByRole('searchbox', { name: 'Search games' }), 'a');
    await expect.poll(() => shelf('Backlog')).toBe(0);
    expect(shelf('Almost done')).toBe(0);
    await userEvent.fill(page.getByRole('searchbox', { name: 'Search games' }), '');
    await expect.poll(() => shelf('Backlog')).toBe(1);
    await userEvent.click(page.getByRole('button', { name: 'Installed', exact: true }));
    await expect.poll(() => shelf('Backlog')).toBe(0);
    expect(shelf('Almost done')).toBe(0);
    await userEvent.click(page.getByRole('button', { name: 'All', exact: true }));
    await expect.poll(() => shelf('Backlog')).toBe(1);
    await userEvent.click(page.getByRole('button', { name: /^Co-op 1$/ }));
    await expect.poll(() => shelf('Backlog')).toBe(0);
    expect(shelf('Almost done')).toBe(0);
  });

  it('Refresh library asks for fresh data, shows busy, then settles', async () => {
    let done!: (v: { games: Game[]; ownedAvailable: boolean }) => void;
    const spy = vi.spyOn(api, 'games').mockImplementation(() => new Promise((r) => (done = r)));
    await render(Library);
    spy.mockClear();
    const b = page.getByRole('button', { name: 'Refresh library' });
    await userEvent.click(b);
    expect(spy).toHaveBeenCalledWith(true);
    await expect.element(b).toHaveAttribute('aria-busy', 'true');
    await expect.element(b).toHaveAttribute('aria-disabled', 'true');
    done({ games: list, ownedAvailable: true });
    await expect.element(b).not.toHaveAttribute('aria-busy', 'true');
    await expect.element(b).not.toHaveAttribute('aria-disabled', 'true');
  });

  it('Refresh library toasts when it fails', async () => {
    const spy = vi.spyOn(api, 'games').mockRejectedValue(new Error('down'));
    await render(Library);
    await render(Toasts);
    spy.mockClear();
    await userEvent.click(page.getByRole('button', { name: 'Refresh library' }));
    await expect.element(page.getByText('Could not refresh the library')).toBeInTheDocument();
  });
});

describe('Library ?open=', () => {
  it('opens the panel of a known game and clears the param', async () => {
    history.replaceState(null, '', '?open=2');
    await render(Library);
    await expect.element(page.getByRole('dialog').getByText('Bravo', { exact: true })).toBeInTheDocument();
    expect(location.search).toBe('');
  });

  it('still opens ?open= when a refresh failed but the old list is there', async () => {
    gamesFailed.set(true);
    history.replaceState(null, '', '?open=2');
    await render(Library);
    await expect.element(page.getByRole('dialog').getByText('Bravo', { exact: true })).toBeInTheDocument();
  });

  it('ignores an unknown ?open= (and clears it)', async () => {
    history.replaceState(null, '', '?open=999');
    await render(Library);
    await expect.element(page.getByRole('button', { name: 'Surprise me' })).toBeInTheDocument();
    expect(page.getByRole('dialog').elements()).toHaveLength(0);
    expect(location.search).toBe('');
  });

  it('mounts filtered by ?tag= (the router remounts Library on each visit)', async () => {
    history.replaceState(null, '', '?tag=auto-coop');
    await render(Library);
    await expect.element(page.getByRole('button', { name: /^Co-op 1$/ })).toHaveAttribute('aria-pressed', 'true');
  });
});
