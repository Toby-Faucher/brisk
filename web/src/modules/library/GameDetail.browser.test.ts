import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, type Game, type GameInfo, type Achievements } from '../../state/api';
import GameDetail from './GameDetail.svelte';

const game: Game = { appid: 620, name: 'Portal 2', installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 0, playtime2wkMin: 0, cover: '', hero: '', logo: '', achUnlocked: null, achTotal: null, running: false };
const off: Game = { ...game, appid: 400, name: 'Portal', installed: false, sizeBytes: 5e9 };
const info: GameInfo = { available: true, description: 'Think with portals & friends.', genres: ['Action', 'Puzzle'], controller: 'full', singleplayer: true, coop: true, remotePlayTogether: true, release: '18 Apr, 2011', metacritic: 95, screenshots: [] };
const none: GameInfo = { available: false, description: '', genres: [], controller: null, singleplayer: false, coop: false, remotePlayTogether: false, release: null, metacritic: null, screenshots: [] };

const ach: Achievements = {
  available: true,
  unlocked: 3,
  total: 5,
  list: [
    { id: 'A', name: 'First Steps', description: 'Take a step.', icon: '', unlockedAt: 1700000100 },
    { id: 'B', name: 'Hidden One', description: '', icon: '', unlockedAt: null },
  ],
};
const noAch: Achievements = { available: false, unlocked: 0, total: 0, list: [] };

const props = (g: Game, extra = {}) => ({ game: g, busy: false, onclose: vi.fn(), onplay: vi.fn(), oninstall: vi.fn(), ...extra });

afterEach(() => vi.restoreAllMocks());
beforeEach(() => vi.spyOn(api, 'gameAchievements').mockResolvedValue(noAch));

describe('GameDetail', () => {
  it('shows 3 of 5 unlocked with a progress bar and the list', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const spy = vi.spyOn(api, 'gameAchievements').mockResolvedValue(ach);
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('3 of 5 unlocked')).toBeInTheDocument();
    await expect.element(page.getByRole('progressbar', { name: '3 of 5 achievements' })).toBeInTheDocument();
    await expect.element(page.getByText('First Steps')).toBeInTheDocument();
    await expect.element(page.getByText('Take a step.')).toBeInTheDocument();
    await expect.element(page.getByText('Hidden One')).toBeInTheDocument();
    expect(spy).toHaveBeenCalledExactlyOnceWith(620);
  });

  it('hides the section when total is 0', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    expect(page.getByText(/unlocked/).elements()).toHaveLength(0);
    expect(page.getByRole('heading', { name: 'Achievements' }).elements()).toHaveLength(0);
  });

  it('shows genres and controller line', async () => {
    const spy = vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByRole('dialog', { name: 'Portal 2' })).toBeInTheDocument();
    await expect.element(page.getByText('Full controller support')).toBeInTheDocument();
    await expect.element(page.getByText('Action', { exact: true })).toBeInTheDocument();
    await expect.element(page.getByText('Puzzle', { exact: true })).toBeInTheDocument();
    await expect.element(page.getByText('Remote Play Together')).toBeInTheDocument();
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    expect(spy).toHaveBeenCalledExactlyOnceWith(620);
  });

  it('shows "No store details" when available is false', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('No store details')).toBeInTheDocument();
  });

  it('Escape closes and returns focus to the card that opened it', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const card = document.body.appendChild(document.createElement('button'));
    card.textContent = 'card';
    card.focus();
    try {
      const p = props(game);
      await render(GameDetail, { props: p });
      const dialog = page.getByRole('dialog', { name: 'Portal 2' });
      await expect.poll(() => dialog.element().contains(document.activeElement)).toBe(true);
      await userEvent.keyboard('{Escape}');
      await expect.element(dialog).not.toBeInTheDocument();
      await expect.poll(() => p.onclose.mock.calls.length).toBe(1);
      expect(document.activeElement).toBe(card);
    } finally {
      card.remove();
    }
  });

  it('Play calls onplay for an installed game', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const p = props(game);
    await render(GameDetail, { props: p });
    await userEvent.click(page.getByRole('button', { name: 'Play', exact: true }));
    expect(p.onplay).toHaveBeenCalledExactlyOnceWith(game);
    expect(p.oninstall).not.toHaveBeenCalled();
  });

  it('Install calls oninstall for a not-installed game', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    const p = props(off);
    await render(GameDetail, { props: p });
    await userEvent.click(page.getByRole('button', { name: 'Install 5.0 GB' }));
    expect(p.oninstall).toHaveBeenCalledExactlyOnceWith(off);
    expect(p.onplay).not.toHaveBeenCalled();
  });

  it('Quit shows only when running', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const r = await render(GameDetail, { props: props(game) });
    await expect.element(page.getByRole('dialog', { name: 'Portal 2' })).toBeInTheDocument();
    expect(page.getByRole('button', { name: /^Quit/ }).elements()).toHaveLength(0);
    await r.rerender({ game: { ...game, running: true } });
    await expect.element(page.getByRole('button', { name: 'Quit Portal 2' })).toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'Resume', exact: true })).toBeInTheDocument();
  });
});
