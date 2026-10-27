import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, type Game } from '../../state/api';
import { games } from '../../state/games';
import { notes, notesLoaded } from '../../state/notes';
import Library from './Library.svelte';

const g = (appid: number, name: string, o: Partial<Game> = {}): Game => ({ appid, name, installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 100, playtime2wkMin: 0, cover: '', hero: '', logo: '', achUnlocked: null, achTotal: null, updated: false, controller: null, coop: false, running: false, ...o });
const list = [g(1, 'Alpha', { playtimeMin: 300 }), g(2, 'Bravo', { playtimeMin: 200 }), g(3, 'Charlie', { installed: false, playtimeMin: 5, coop: true })];

afterEach(() => vi.restoreAllMocks());
beforeEach(() => {
  notes.set({ tags: {}, games: {} });
  notesLoaded.set(true);
  games.set({ games: list, ownedAvailable: true });
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

  it('lists smart tags that match with counts, and filters by one', async () => {
    await render(Library);
    await expect.element(page.getByRole('button', { name: /^Co-op 1$/ })).toBeInTheDocument();
    expect(page.getByRole('button', { name: /^Controller/ }).elements()).toHaveLength(0);
    await userEvent.click(page.getByRole('button', { name: /^Co-op 1$/ }));
    await expect.element(page.getByRole('button', { name: /^Charlie/ })).toBeInTheDocument();
    expect(page.getByRole('button', { name: /^Alpha/ }).elements()).toHaveLength(0);
  });
});
