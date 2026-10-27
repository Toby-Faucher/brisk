import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { prefs } from '../../state/prefs';
import { api, type SessionRec, type Game, type GameInfo, type Achievements, type GameLocal } from '../../state/api';
import GameDetail from './GameDetail.svelte';

const game: Game = { appid: 620, name: 'Portal 2', installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 0, playtime2wkMin: 0, cover: '', hero: '', logo: '', achUnlocked: null, achTotal: null, updated: false, controller: null, coop: false, running: false };
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

afterEach(() => {
  vi.restoreAllMocks();
  prefs.set({});
});
beforeEach(() => {
  vi.spyOn(api, 'gameAchievements').mockResolvedValue(noAch);
  vi.spyOn(api, 'gameNews').mockResolvedValue({ items: [] });
  vi.spyOn(api, 'gameLocal').mockResolvedValue({ screenshots: [], saves: null });
  vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: [] });
  vi.spyOn(api, 'prefs').mockImplementation(async () => ({ prefs: get(prefs), homeOrder: [] }));
  vi.spyOn(api, 'notes').mockResolvedValue({ tags: {}, games: {} });
});

const local: GameLocal = { screenshots: ['b.png', 'a.jpg'], saves: { files: 14, bytes: 2_100_000, modified: Math.floor(Date.now() / 1000) - 3 * 86_400 } };
// a fetch-made object URL, as api.screenshot returns
const blobUrl = () => URL.createObjectURL(new Blob([new Uint8Array([137, 80, 78, 71])], { type: 'image/png' }));

describe('GameDetail', () => {
  it('renders news items that share date and title', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const it = { title: 'Same', url: 'https://n.example.test/a', date: 5, patch: true };
    vi.spyOn(api, 'gameNews').mockResolvedValue({ items: [it, { ...it }] });
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByRole('link', { name: 'Same' }).first()).toBeInTheDocument();
    expect(page.getByRole('link', { name: 'Same' }).elements()).toHaveLength(2);
  });

  it('links only https news URLs', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'gameNews').mockResolvedValue({
      items: [
        { title: 'Safe patch', url: 'https://n.example.test/a', date: 2, patch: true },
        { title: 'No link <b>x</b>', url: '', date: 1, patch: false },
      ],
    });
    await render(GameDetail, { props: props(game) });
    const a = page.getByRole('link', { name: 'Safe patch' });
    await expect.element(a).toHaveAttribute('href', 'https://n.example.test/a');
    await expect.element(a).toHaveAttribute('rel', 'noopener noreferrer');
    await expect.element(a).toHaveAttribute('target', '_blank');
    await expect.element(page.getByText('No link <b>x</b>')).toBeInTheDocument();
    expect(page.getByRole('link').elements().filter((e) => e.textContent?.includes('No link'))).toHaveLength(0);
  });

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

  it('screenshots load through fetch with the secret header, not img src', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    vi.spyOn(api, 'gameLocal').mockResolvedValue(local);
    const shot = vi.spyOn(api, 'screenshot').mockImplementation(async () => blobUrl());
    await render(GameDetail, { props: props(game) });
    const img = page.getByRole('img', { name: 'Portal 2 screenshot 1 from this PC' });
    await expect.element(img).toHaveAttribute('src', expect.stringMatching(/^blob:/));
    await expect.poll(() => shot.mock.calls.length).toBe(2);
    expect(shot).toHaveBeenCalledWith(620, 'b.png');
    expect(shot).toHaveBeenCalledWith(620, 'a.jpg');
    expect(page.getByRole('img').elements().every((e) => (e as HTMLImageElement).src.startsWith('blob:'))).toBe(true);
    await expect.element(page.getByText('Cloud saves: 14 files, 2.1 MB, synced 3 d ago')).toBeInTheDocument();
  });

  it('object URLs are revoked on close', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    vi.spyOn(api, 'gameLocal').mockResolvedValue(local);
    const made: string[] = [];
    vi.spyOn(api, 'screenshot').mockImplementation(async () => {
      const u = blobUrl();
      made.push(u);
      return u;
    });
    const revoke = vi.spyOn(URL, 'revokeObjectURL');
    const p = props(game);
    const r = await render(GameDetail, { props: p });
    await expect.poll(() => made.length).toBe(2);
    await expect.element(page.getByRole('img', { name: 'Portal 2 screenshot 2 from this PC' })).toHaveAttribute('src', expect.stringMatching(/^blob:/));
    await userEvent.keyboard('{Escape}');
    await expect.poll(() => p.onclose.mock.calls.length).toBe(1);
    r.unmount(); // what the Library does on onclose
    for (const u of made) expect(revoke).toHaveBeenCalledWith(u);
  });

  it('no screenshots section or saves line when the host has none', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    expect(page.getByRole('heading', { name: 'Your screenshots' }).elements()).toHaveLength(0);
    expect(page.getByText(/Cloud saves/).elements()).toHaveLength(0);
  });

  it('shows the line and Use Balanced calls putPrefs with the saved codec', async () => {
    const now = Date.now();
    const rec = (n: number): SessionRec => ({ appid: 620, name: 'Portal 2', started: now - n * 86_400_000, ended: now - n * 86_400_000 + 3_600_000, host: 'h', preset: 'crisp', stats: { sent: 1000, dropped: 50 } as SessionRec['stats'] });
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: [rec(3), rec(4), rec(5), { ...rec(6), stats: null }] });
    const saved = { preset: 'balanced', codec: 'hevc', updated: 1 } as const;
    const put = vi.spyOn(api, 'putPrefs').mockResolvedValue(saved);
    prefs.set({ 620: { preset: 'crisp', codec: 'hevc', updated: 0 } });
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Streamed 4 times · 4 h · last 3 d ago · usually Crisp')).toBeInTheDocument();
    await expect.element(page.getByText(/Streaming was rough in 3 of the last 4 sessions \(dropped frames or a failed check\)\. Try Balanced\?/)).toBeInTheDocument();
    expect(put).not.toHaveBeenCalled();
    await userEvent.click(page.getByRole('button', { name: 'Use Balanced' }));
    expect(put).toHaveBeenCalledWith(620, { preset: 'balanced', codec: 'hevc' });
    await vi.waitFor(() => expect(get(prefs)[620]).toEqual(saved));
    await expect.element(page.getByRole('button', { name: 'Use Balanced' })).not.toBeInTheDocument();
  });

  it('hides both when the game was never streamed', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    expect(page.getByText(/Streamed/).elements()).toHaveLength(0);
    expect(page.getByRole('button', { name: /Use / }).elements()).toHaveLength(0);
  });

  const mk = (n: number, o: Partial<SessionRec> = {}): SessionRec => ({ appid: 620, name: 'Portal 2', started: 1_000_000 - n, ended: 1_000_000 - n + 60_000, host: 'h', preset: 'crisp', stats: { sent: 1000, dropped: 50 } as SessionRec['stats'], ...o });
  const three = [mk(1), mk(2), mk(3)];
  const sugg = () => page.getByRole('button', { name: 'Use Balanced' });

  it('keeps a saved hevc and falls back to h264 without a saved codec', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: three });
    const put = vi.spyOn(api, 'putPrefs').mockResolvedValue({ preset: 'balanced', codec: 'h264', updated: 1 });
    await render(GameDetail, { props: props(game) });
    await userEvent.click(sugg());
    await vi.waitFor(() => expect(put).toHaveBeenCalledWith(620, { preset: 'balanced', codec: 'h264' }));
  });

  it('toasts and keeps the button on a failed save', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: three });
    const put = vi.spyOn(api, 'putPrefs').mockRejectedValue(new Error('x'));
    await render(GameDetail, { props: props(game) });
    await userEvent.click(sugg());
    await vi.waitFor(() => expect(put).toHaveBeenCalledTimes(1));
    await expect.element(sugg()).toBeInTheDocument();
    expect(get(prefs)[620]).toBeUndefined();
  });

  it('hides the suggestion when Balanced is already saved', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: three });
    prefs.set({ 620: { preset: 'balanced', codec: 'h264', updated: 0 } });
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText(/Streamed 3 times/)).toBeInTheDocument();
    expect(page.getByText(/Streaming was rough/).elements()).toHaveLength(0);
  });
});
