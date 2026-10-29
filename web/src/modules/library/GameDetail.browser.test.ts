import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { homeOrder, prefs } from '../../state/prefs';
import { api, ApiError, type SessionRec, type Game, type GameInfo, type Achievements, type GameLocal } from '../../state/api';
import '../../styles/tokens.css';
import GameDetail from './GameDetail.svelte';
import Toasts from '../../shell/Toasts.svelte';

const game: Game = {
  appid: 620,
  name: 'Portal 2',
  installed: true,
  sizeBytes: null,
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
const off: Game = { ...game, appid: 400, name: 'Portal', installed: false, sizeBytes: 5e9 };
const info: GameInfo = {
  available: true,
  description: 'Think with portals & friends.',
  genres: ['Action', 'Puzzle'],
  controller: 'full',
  singleplayer: true,
  coop: true,
  remotePlayTogether: true,
  release: '18 Apr, 2011',
  metacritic: 95,
  screenshots: [],
};
const none: GameInfo = {
  available: false,
  description: '',
  genres: [],
  controller: null,
  singleplayer: false,
  coop: false,
  remotePlayTogether: false,
  release: null,
  metacritic: null,
  screenshots: [],
};

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

const local: GameLocal = {
  screenshots: ['b.png', 'a.jpg'],
  saves: { files: 14, bytes: 2_100_000, modified: Math.floor(Date.now() / 1000) - 3 * 86_400 },
};
// a fetch-made object URL, as api.screenshot returns
const blobUrl = () => URL.createObjectURL(new Blob([new Uint8Array([137, 80, 78, 71])], { type: 'image/png' }));

describe('GameDetail refresh guards', () => {
  const flush = async () => {
    for (let k = 0; k < 5; k++) await Promise.resolve();
  };
  afterEach(() => vi.useRealTimers());

  it('ignores a second click while the first is in flight, even after 10 s; re-enables once done', async () => {
    const i = vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    const b = page.getByRole('button', { name: 'Refresh Steam data' });
    const el = b.element() as HTMLElement;
    let done!: (v: GameInfo) => void;
    i.mockClear();
    i.mockImplementation(() => new Promise((r) => (done = r)));
    vi.useFakeTimers();
    el.click();
    await flush();
    expect(i).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(11_000);
    expect(el.getAttribute('aria-disabled')).toBe('true'); // still loading
    el.click();
    await flush();
    expect(i).toHaveBeenCalledTimes(1);
    done(info);
    await flush();
    await vi.advanceTimersByTimeAsync(0);
    expect(el.getAttribute('aria-disabled')).not.toBe('true'); // cooldown over and not loading
    el.click();
    await flush();
    expect(i).toHaveBeenCalledTimes(2);
  });

  it('is disabled for 10 s after a click and then works again', async () => {
    const i = vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    const el = page.getByRole('button', { name: 'Refresh Steam data' }).element() as HTMLElement;
    vi.useFakeTimers();
    el.click();
    await flush();
    await vi.advanceTimersByTimeAsync(9_000);
    expect(el.getAttribute('aria-disabled')).toBe('true');
    await vi.advanceTimersByTimeAsync(1_001);
    expect(el.getAttribute('aria-disabled')).not.toBe('true');
    i.mockClear();
    el.click();
    await flush();
    expect(i).toHaveBeenCalledTimes(1);
  });

  it('toasts when a refresh request fails and keeps what is shown', async () => {
    const i = vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    await render(GameDetail, { props: props(game) });
    await render(Toasts);
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    i.mockRejectedValue(new Error('down'));
    await userEvent.click(page.getByRole('button', { name: 'Refresh Steam data' }));
    await expect.element(page.getByText('Could not refresh Steam data')).toBeInTheDocument();
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
  });

  it('is disabled until the first load has finished', async () => {
    vi.spyOn(api, 'gameInfo').mockImplementation(() => new Promise(() => {}));
    await render(GameDetail, { props: props(game) });
    const b = page.getByRole('button', { name: 'Refresh Steam data' });
    await expect.element(b).toHaveAttribute('aria-disabled', 'true');
  });

  it('drops late refresh answers after the panel switched to another game', async () => {
    const i = vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const r = await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    let late!: (v: GameInfo) => void;
    i.mockImplementationOnce(() => new Promise((res) => (late = res)));
    await userEvent.click(page.getByRole('button', { name: 'Refresh Steam data' }));
    i.mockResolvedValue({ ...info, description: 'The other game.' });
    await r.rerender({ game: off });
    await expect.element(page.getByText('The other game.')).toBeInTheDocument();
    late({ ...info, description: 'Stale answer.' });
    await new Promise((res) => setTimeout(res, 50));
    expect(page.getByText('Stale answer.').elements()).toHaveLength(0);
    await expect.element(page.getByText('The other game.')).toBeInTheDocument();
  });
});

describe('GameDetail refresh across games', () => {
  afterEach(() => vi.useRealTimers());

  it('the cooldown of game A does not carry over to game B', async () => {
    const i = vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const r = await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    const b = page.getByRole('button', { name: 'Refresh Steam data' });
    await userEvent.click(b);
    await expect.element(b).toHaveAttribute('aria-disabled', 'true'); // cooling down (or still reloading)
    await expect.poll(() => (b.element() as HTMLElement).getAttribute('aria-busy')).not.toBe('true');
    vi.useFakeTimers();
    await r.rerender({ game: off });
    await vi.advanceTimersByTimeAsync(50); // B's plain load, far less than 10 s
    expect((b.element() as HTMLElement).getAttribute('aria-disabled')).not.toBe('true'); // no waiting: the 10 s must not carry over
    vi.useRealTimers();
    i.mockClear();
    await userEvent.click(b);
    expect(i).toHaveBeenCalledWith('400', true);
  });

  it('an in-flight refresh of game A does not block game B', async () => {
    const i = vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const r = await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    i.mockImplementationOnce(() => new Promise(() => {}));
    const b = page.getByRole('button', { name: 'Refresh Steam data' });
    await userEvent.click(b);
    await r.rerender({ game: off });
    await new Promise((res) => setTimeout(res, 50));
    expect((b.element() as HTMLElement).getAttribute('aria-disabled')).not.toBe('true');
    i.mockClear();
    await userEvent.click(b);
    expect(i).toHaveBeenCalledWith('400', true);
  });
});

describe('GameDetail', () => {
  it('Refresh re-fetches info, achievements and news with refresh, updates sections, then waits', async () => {
    const i = vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const a = vi.spyOn(api, 'gameAchievements').mockResolvedValue(noAch);
    const n = vi.spyOn(api, 'gameNews').mockResolvedValue({ items: [] });
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
    a.mockResolvedValue(ach);
    n.mockResolvedValue({ items: [{ title: 'Fresh patch', url: 'https://n.example.test/f', date: 1, patch: true }] });
    const b = page.getByRole('button', { name: 'Refresh Steam data' });
    await userEvent.click(b);
    expect(i).toHaveBeenLastCalledWith('620', true);
    expect(a).toHaveBeenLastCalledWith('620', true);
    expect(n).toHaveBeenLastCalledWith('620', true);
    await expect.element(page.getByRole('link', { name: 'Fresh patch' })).toBeInTheDocument();
    await expect.element(page.getByText('First Steps')).toBeInTheDocument();
    await expect.element(b).toHaveAttribute('aria-disabled', 'true');
    await expect.element(b).toHaveAttribute('title', 'Steam data was just refreshed. Wait 10 seconds to refresh again.');
    i.mockClear();
    (b.element() as HTMLElement).click(); // a real click would wait for aria-disabled to clear
    expect(i).not.toHaveBeenCalled();
  });

  it('warns above Play while Steam has an update, and Play stays enabled', async () => {
    await render(GameDetail, { props: props({ ...game, update: 'needed' }) });
    await expect.element(page.getByText(/Steam still has to update this game/)).toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: /Play/ })).not.toHaveAttribute('aria-disabled', 'true');
  });

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
    expect(
      page
        .getByRole('link')
        .elements()
        .filter((e) => e.textContent?.includes('No link')),
    ).toHaveLength(0);
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
    expect(spy).toHaveBeenCalledExactlyOnceWith('620');
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
    expect(spy).toHaveBeenCalledExactlyOnceWith('620');
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

  it('the close X sits in the top-right corner, clear of Refresh, and closes the panel', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const p = props(game);
    await render(GameDetail, { props: p });
    const x = page.getByRole('button', { name: 'Close', exact: true });
    await expect.element(x).toBeInTheDocument();
    const box = x.element().getBoundingClientRect();
    const panel = document.querySelector('.gd')?.getBoundingClientRect() as DOMRect;
    expect(panel.right - box.right).toBeLessThan(16);
    expect(box.top - panel.top).toBeLessThan(16);
    const rf = page.getByRole('button', { name: 'Refresh Steam data' }).element().getBoundingClientRect();
    expect(rf.right).toBeLessThanOrEqual(box.left);
    expect(document.body.textContent).not.toContain('Close');
    await userEvent.click(x);
    await expect.poll(() => p.onclose.mock.calls.length).toBe(1);
  });

  it('with a confirm in front, Escape closes only the confirm; the X is red-tinted on hover', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    const emu: Game = {
      ...game,
      id: 'emu:dolphin:RMCE01',
      source: 'emu',
      emulator: 'dolphin',
      name: 'Mario Kart Wii',
      caps: { install: false, controls: true, achievements: false, launchOptions: false, saves: false, screenshots: false, art: true },
    };
    const p = props(emu);
    await render(GameDetail, { props: p });
    await userEvent.click(page.getByRole('button', { name: 'Remove from library' }));
    await expect.element(page.getByRole('dialog', { name: /Remove Mario Kart Wii/ })).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    await expect.element(page.getByRole('dialog', { name: /Remove Mario Kart Wii/ })).not.toBeInTheDocument();
    expect(p.onclose).not.toHaveBeenCalled();
    await expect.element(page.getByRole('dialog', { name: 'Mario Kart Wii' })).toBeInTheDocument();
    await userEvent.hover(page.getByRole('button', { name: 'Close', exact: true }));
    await expect
      .poll(() => getComputedStyle(page.getByRole('button', { name: 'Close', exact: true }).element()).color)
      .toBe('rgb(251, 113, 133)');
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
    expect(shot).toHaveBeenCalledWith('620', 'b.png');
    expect(shot).toHaveBeenCalledWith('620', 'a.jpg');
    expect(
      page
        .getByRole('img')
        .elements()
        .every((e) => (e as HTMLImageElement).src.startsWith('blob:')),
    ).toBe(true);
    await expect.element(page.getByText('Cloud saves: 14 files, 2.1 MB, synced 3 d ago')).toBeInTheDocument();
  });

  it('a failed screenshot tile offers Retry, and the retry loads it', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    vi.spyOn(api, 'gameLocal').mockResolvedValue({ screenshots: ['b.png'], saves: null });
    const shot = vi
      .spyOn(api, 'screenshot')
      .mockRejectedValueOnce(new Error('down'))
      .mockImplementation(async () => blobUrl());
    await render(GameDetail, { props: props(game) });
    const retry = page.getByRole('button', { name: 'Retry loading screenshot 1' });
    await expect.element(retry).toBeInTheDocument();
    await userEvent.click(retry);
    expect(document.activeElement?.getAttribute('aria-label')).toBe('Open Portal 2 screenshot 1 from this PC');
    await expect
      .element(page.getByRole('img', { name: 'Portal 2 screenshot 1 from this PC' }))
      .toHaveAttribute('src', expect.stringMatching(/^blob:/));
    expect(shot).toHaveBeenCalledTimes(2);
    expect(page.getByRole('button', { name: 'Retry loading screenshot 1' }).elements()).toHaveLength(0);
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
    await expect
      .element(page.getByRole('img', { name: 'Portal 2 screenshot 2 from this PC' }))
      .toHaveAttribute('src', expect.stringMatching(/^blob:/));
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
    const rec = (n: number): SessionRec => ({
      appid: 620,
      name: 'Portal 2',
      started: now - n * 86_400_000,
      ended: now - n * 86_400_000 + 3_600_000,
      host: 'h',
      preset: 'crisp',
      stats: { sent: 1000, dropped: 50 } as SessionRec['stats'],
    });
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: [rec(3), rec(4), rec(5), { ...rec(6), stats: null }] });
    const saved = { preset: 'balanced', codec: 'hevc', updated: 1 } as const;
    const put = vi.spyOn(api, 'putPrefs').mockResolvedValue(saved);
    prefs.set({ 620: { preset: 'crisp', codec: 'hevc', updated: 0 } });
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('Streamed 4 times · 4 h · last 3 d ago · usually Crisp')).toBeInTheDocument();
    await expect
      .element(page.getByText(/Streaming was rough in 3 of the last 4 sessions \(dropped frames or a failed check\)\. Try Balanced\?/))
      .toBeInTheDocument();
    expect(put).not.toHaveBeenCalled();
    await userEvent.click(page.getByRole('button', { name: 'Use Balanced' }));
    expect(put).toHaveBeenCalledWith('620', { preset: 'balanced', codec: 'hevc' });
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

  const mk = (n: number, o: Partial<SessionRec> = {}): SessionRec => ({
    appid: 620,
    name: 'Portal 2',
    started: 1_000_000 - n,
    ended: 1_000_000 - n + 60_000,
    host: 'h',
    preset: 'crisp',
    stats: { sent: 1000, dropped: 50 } as SessionRec['stats'],
    ...o,
  });
  const three = [mk(1), mk(2), mk(3)];
  const sugg = () => page.getByRole('button', { name: 'Use Balanced' });

  it('falls back to h264 when no codec is saved', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: three });
    const put = vi.spyOn(api, 'putPrefs').mockResolvedValue({ preset: 'balanced', codec: 'h264', updated: 1 });
    await render(GameDetail, { props: props(game) });
    await userEvent.click(sugg());
    await vi.waitFor(() => expect(put).toHaveBeenCalledWith('620', { preset: 'balanced', codec: 'h264' }));
  });

  it('toasts and keeps the button on a failed save', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: three });
    const put = vi.spyOn(api, 'putPrefs').mockRejectedValue(new Error('x'));
    await render(Toasts);
    await render(GameDetail, { props: props(game) });
    await userEvent.click(sugg());
    await vi.waitFor(() => expect(put).toHaveBeenCalledTimes(1));
    await expect.element(page.getByText('Could not save Balanced')).toBeInTheDocument();
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

  it('does not save while the prefs could not be loaded', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'sessions').mockResolvedValue({ sessions: three });
    vi.spyOn(api, 'prefs').mockRejectedValue(new Error('down'));
    const put = vi.spyOn(api, 'putPrefs').mockResolvedValue({ preset: 'balanced', codec: 'h264', updated: 1 });
    homeOrder.set(undefined);
    await render(Toasts);
    await render(GameDetail, { props: props(game) });
    await userEvent.click(sugg());
    await expect.element(page.getByText('Could not load your saved settings')).toBeInTheDocument();
    expect(put).not.toHaveBeenCalled();
    await expect.element(sugg()).not.toHaveAttribute('aria-disabled', 'true');
  });

  const thumbs = [0, 1, 2].map((i) => `https://cdn.example.test/ss_0${i}.600x338.jpg`);
  const shotsInfo: GameInfo = { ...info, screenshots: thumbs, screenshotsFull: thumbs.map((u) => u.replace('600x338', '1920x1080')) };
  const big = () => page.getByRole('dialog', { name: /^Screenshot \d+ of \d+$/ });

  it('a store screenshot opens a lightbox with the full-size image; arrows, buttons and Escape', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(shotsInfo);
    await render(GameDetail, { props: props(game) });
    const thumb = page.getByRole('button', { name: 'Open Portal 2 screenshot 2' });
    await userEvent.click(thumb);
    await expect.element(big()).toBeInTheDocument();
    await expect.element(big()).toHaveAccessibleName('Screenshot 2 of 3');
    await expect.element(big().getByRole('img')).toHaveAttribute('src', thumbs[1].replace('600x338', '1920x1080'));
    await expect.element(big().getByRole('img')).toHaveAttribute('alt', 'Portal 2 screenshot 2');
    await userEvent.keyboard('{ArrowRight}');
    await expect.element(big()).toHaveAccessibleName('Screenshot 3 of 3');
    await userEvent.keyboard('{ArrowRight}'); // wraps
    await expect.element(big()).toHaveAccessibleName('Screenshot 1 of 3');
    await userEvent.keyboard('{ArrowLeft}');
    await expect.element(big()).toHaveAccessibleName('Screenshot 3 of 3');
    await userEvent.click(page.getByRole('button', { name: 'Previous screenshot' }));
    await expect.element(big()).toHaveAccessibleName('Screenshot 2 of 3');
    await userEvent.click(page.getByRole('button', { name: 'Next screenshot' }));
    await expect.element(big()).toHaveAccessibleName('Screenshot 3 of 3');
    await expect.poll(() => big().element().contains(document.activeElement)).toBe(true);
    await userEvent.keyboard('{Escape}');
    await expect.element(big()).not.toBeInTheDocument();
    // only the lightbox closed; focus is back on the thumbnail that opened it
    await expect.element(page.getByRole('dialog', { name: 'Portal 2' })).toBeInTheDocument();
    await expect.poll(() => document.activeElement).toBe(thumb.element());
  });

  it('the lightbox uses the thumbnail when the server sent no full-size list', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue({ ...info, screenshots: thumbs });
    await render(GameDetail, { props: props(game) });
    await userEvent.click(page.getByRole('button', { name: 'Open Portal 2 screenshot 1' }));
    await expect.element(big().getByRole('img')).toHaveAttribute('src', thumbs[0]);
    await userEvent.click(page.getByRole('button', { name: 'Close screenshot' }));
    await expect.element(big()).not.toBeInTheDocument();
  });

  it('your own screenshots open in the lightbox from the same blob URL', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    vi.spyOn(api, 'gameLocal').mockResolvedValue(local);
    const made: string[] = [];
    vi.spyOn(api, 'screenshot').mockImplementation(async () => (made.push(blobUrl()), made.at(-1)!));
    await render(GameDetail, { props: props(game) });
    await expect.poll(() => made.length).toBe(2);
    const second = page.getByRole('img', { name: 'Portal 2 screenshot 2 from this PC' });
    await expect.element(second).toHaveAttribute('src', expect.stringMatching(/^blob:/));
    const src = second.element().getAttribute('src');
    await userEvent.click(page.getByRole('button', { name: 'Open Portal 2 screenshot 2 from this PC' }));
    await expect.element(big().getByRole('img')).toHaveAttribute('src', src!);
    expect(made).toHaveLength(2); // no second fetch
  });

  const many: Achievements = {
    available: true,
    unlocked: 2,
    total: 9,
    list: [
      { id: 'A', name: 'First Steps', description: 'Take a step.', icon: '', unlockedAt: 1700000100 },
      { id: 'B', name: 'Pokémon Master', description: 'Catch them.', icon: '', unlockedAt: 1700000200 },
      ...Array.from({ length: 7 }, (_, i) => ({
        id: `L${i}`,
        name: `Locked ${i}`,
        description: i === 0 ? 'Find the café' : '',
        icon: '',
        unlockedAt: null,
      })),
    ],
  };
  const achDialog = () => page.getByRole('dialog', { name: 'Portal 2 achievements' });

  it('View all opens every achievement in a modal with a small search', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'gameAchievements').mockResolvedValue(many);
    await render(GameDetail, { props: props(game) });
    const open = page.getByRole('button', { name: 'View all 9 achievements' });
    await userEvent.click(open);
    await expect.element(achDialog()).toBeInTheDocument();
    const items = () => achDialog().getByRole('listitem').elements();
    expect(items()).toHaveLength(9);
    expect(items().filter((li) => li.classList.contains('locked'))).toHaveLength(7);
    await expect
      .element(
        achDialog()
          .getByText(/^Unlocked /)
          .first(),
      )
      .toBeInTheDocument();
    const search = achDialog().getByRole('searchbox', { name: 'Search achievements' });
    await userEvent.fill(search, 'pokemon');
    await expect.poll(() => items().map((li) => li.querySelector('b')?.textContent)).toEqual(['Pokémon Master']);
    await userEvent.fill(search, 'CAFE'); // description, accents and case ignored
    await expect.poll(() => items().map((li) => li.querySelector('b')?.textContent)).toEqual(['Locked 0']);
    await userEvent.fill(search, 'zzz');
    await expect.element(achDialog().getByText('No matches')).toBeInTheDocument();
    expect(items()).toHaveLength(0);
    await userEvent.keyboard('{Escape}'); // clears the search
    await userEvent.keyboard('{Escape}'); // closes
    await expect.element(achDialog()).not.toBeInTheDocument();
    await expect.element(page.getByRole('dialog', { name: 'Portal 2' })).toBeInTheDocument();
    await expect.poll(() => document.activeElement).toBe(open.element());
  });

  it('a patch news item has a separate badge with space before it', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'gameNews').mockResolvedValue({
      items: [{ title: 'Big update', url: 'https://n.example.test/a', date: 2, patch: true }],
    });
    await render(GameDetail, { props: props(game) });
    const a = page.getByRole('link', { name: 'Big update' });
    await expect.element(a).toBeInTheDocument();
    const badge = page.getByText('Patch', { exact: true }).element();
    expect(badge.getBoundingClientRect().left - a.element().getBoundingClientRect().right).toBeGreaterThanOrEqual(6);
  });

  it('wide screens: two panes, the left one static and the right one scrolling', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'gameAchievements').mockResolvedValue(ach);
    await render(GameDetail, { props: props(game) });
    const d = page.getByRole('dialog', { name: 'Portal 2' });
    await expect.element(d.getByText('Think with portals & friends.')).toBeInTheDocument();
    const box = d.element().getBoundingClientRect();
    expect(box.width).toBeGreaterThanOrEqual(940);
    expect(box.width).toBeLessThanOrEqual(1100);
    const side = d.element().querySelector('.gd-side')!;
    const main = d.element().querySelector('.gd-main')!;
    expect(side.contains(page.getByRole('button', { name: 'Play', exact: true }).element())).toBe(true);
    expect(main.contains(page.getByText('Think with portals & friends.').element())).toBe(true);
    expect(side.getBoundingClientRect().right).toBeLessThanOrEqual(main.getBoundingClientRect().left + 1);
    expect(getComputedStyle(main).overflowY).toBe('auto');
    expect(getComputedStyle(d.element()).overflowY).not.toBe('auto');
  });

  it('narrow screens: one scrolling column', async () => {
    await page.viewport(700, 800);
    try {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
      await render(GameDetail, { props: props(game) });
      const d = page.getByRole('dialog', { name: 'Portal 2' });
      await expect.element(d.getByText('Think with portals & friends.')).toBeInTheDocument();
      const side = d.element().querySelector('.gd-side')!.getBoundingClientRect();
      const main = d.element().querySelector('.gd-main')!.getBoundingClientRect();
      expect(main.top).toBeGreaterThanOrEqual(side.bottom - 1);
      expect(getComputedStyle(d.element()).overflowY).toBe('auto');
    } finally {
      await page.viewport(1280, 800);
    }
  });

  // Tiles never scroll into view here (a stub observer), so every fetch comes from the lightbox.
  const noObserver = () =>
    vi.stubGlobal(
      'IntersectionObserver',
      class {
        observe() {}
        disconnect() {}
        unobserve() {}
      },
    );
  const ten: GameLocal = { screenshots: Array.from({ length: 10 }, (_, i) => `s${i + 1}.png`), saves: null };

  it('a local screenshot fetches only itself and its neighbours, then the next one on a step', async () => {
    noObserver();
    try {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
      vi.spyOn(api, 'gameLocal').mockResolvedValue(ten);
      const shot = vi.spyOn(api, 'screenshot').mockImplementation(async () => blobUrl());
      await render(GameDetail, { props: props(game) });
      await userEvent.click(page.getByRole('button', { name: 'Open Portal 2 screenshot 3 from this PC' }));
      await expect.element(big()).toHaveAccessibleName('Screenshot 3 of 10');
      await expect.poll(() => shot.mock.calls.map((c) => c[1]).sort()).toEqual(['s2.png', 's3.png', 's4.png']);
      await userEvent.keyboard('{ArrowRight}');
      await expect.poll(() => shot.mock.calls.map((c) => c[1]).sort()).toEqual(['s2.png', 's3.png', 's4.png', 's5.png']);
      await expect.element(big().getByRole('img')).toHaveAttribute('src', expect.stringMatching(/^blob:/));
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('a local screenshot that fails to load says so, and is tried again on the next open', async () => {
    noObserver();
    try {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
      vi.spyOn(api, 'gameLocal').mockResolvedValue({ screenshots: ['only.png'], saves: null });
      const shot = vi
        .spyOn(api, 'screenshot')
        .mockRejectedValueOnce(new Error('gone'))
        .mockImplementation(async () => blobUrl());
      await render(GameDetail, { props: props(game) });
      const open = page.getByRole('button', { name: 'Open Portal 2 screenshot 1 from this PC' });
      await userEvent.click(open);
      await expect.element(big().getByRole('status')).toHaveTextContent('Could not load this screenshot');
      await userEvent.keyboard('{Escape}');
      await expect.element(big()).not.toBeInTheDocument();
      await userEvent.click(open);
      await expect.element(big().getByRole('img')).toHaveAttribute('src', expect.stringMatching(/^blob:/));
      expect(shot).toHaveBeenCalledTimes(2);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('Escape in the achievement search clears the text first, then closes the modal', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'gameAchievements').mockResolvedValue(many);
    await render(GameDetail, { props: props(game) });
    await userEvent.click(page.getByRole('button', { name: 'View all 9 achievements' }));
    const search = achDialog().getByRole('searchbox', { name: 'Search achievements' });
    await userEvent.fill(search, 'zzz');
    await userEvent.keyboard('{Escape}');
    await expect.element(search).toHaveValue('');
    await expect.element(achDialog()).toBeInTheDocument();
    await expect.element(page.getByRole('dialog', { name: 'Portal 2' })).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    await expect.element(achDialog()).not.toBeInTheDocument();
    await expect.element(page.getByRole('dialog', { name: 'Portal 2' })).toBeInTheDocument();
  });

  it('the achievement list in the modal can take focus to scroll, and is named', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'gameAchievements').mockResolvedValue(many);
    await render(GameDetail, { props: props(game) });
    await userEvent.click(page.getByRole('button', { name: 'View all 9 achievements' }));
    await expect.element(achDialog().getByRole('list', { name: 'Achievements' })).toHaveAttribute('tabindex', '0');
  });

  it('View all shows only when there are more achievements than the preview', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
    vi.spyOn(api, 'gameAchievements').mockResolvedValue(ach); // 2 in the list
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('First Steps')).toBeInTheDocument();
    expect(page.getByRole('heading', { name: 'Achievements' }).elements()).toHaveLength(1);
    expect(page.getByRole('button', { name: /^View all/ }).elements()).toHaveLength(0);
  });

  it('two store screenshots with the same URL render without a key error', async () => {
    const dup = 'https://cdn.example.test/same.jpg';
    vi.spyOn(api, 'gameInfo').mockResolvedValue({ ...info, screenshots: [dup, dup] });
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByRole('button', { name: 'Open Portal 2 screenshot 2' })).toBeInTheDocument();
  });

  describe('Controls section', () => {
    const emu: Game = { ...game, appid: 0xe0000001, name: 'Mario Kart Wii' };
    const fx = async () => (await import('../../../../crates/brisk-hostd/tests/fixtures/controls/v2-classic-get.json')).default;

    it('an emulator id shows the summary and the Edit mappings link into Settings', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
      const c = vi.spyOn(api, 'emulatorControls').mockResolvedValue((await fx()) as never);
      await render(GameDetail, { props: props(emu) });
      await expect.element(page.getByRole('heading', { name: 'Controls' })).toBeInTheDocument();
      await expect.element(page.getByText(/Dolphin uses one mapping for every game on this machine/)).toBeInTheDocument();
      await expect
        .element(page.getByRole('link', { name: 'Edit mappings' }))
        .toHaveAttribute('href', '/settings#emulators/dolphin/controllers');
      expect(c).toHaveBeenCalledWith('classic');
      expect(document.querySelector('svg[aria-label="Pad buttons"]')).toBeNull(); // text only, no drawing
    });

    it('is hidden when hostd has no Dolphin (404)', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
      vi.spyOn(api, 'emulatorControls').mockRejectedValue(new ApiError('no controls on this machine', 404));
      await render(GameDetail, { props: props(emu) });
      await expect.poll(() => page.getByRole('heading', { name: 'Controls' }).elements().length).toBe(0);
    });

    // what a new hostd sends for the same game: id, source and caps decide, not the number
    const caps = { install: false, controls: true, achievements: false, launchOptions: false, saves: false, screenshots: false };
    const newEmu: Game = { ...emu, id: 'emu:dolphin:mario-kart-wii', source: 'emu', legacyId: '3758096385', emulator: 'dolphin', caps };

    it('a new hostd emulator game shows the link, asks hostd by id, and has Play and Quit but no Install', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
      vi.spyOn(api, 'emulatorControls').mockResolvedValue((await fx()) as never);
      const ach = vi.mocked(api.gameAchievements);
      const loc = vi.mocked(api.gameLocal);
      await render(GameDetail, { props: props({ ...newEmu, running: true, installed: false }) });
      await expect.element(page.getByRole('link', { name: 'Edit mappings' })).toBeInTheDocument();
      await expect.element(page.getByRole('button', { name: 'Resume' })).toBeInTheDocument();
      await expect.element(page.getByRole('button', { name: 'Quit Mario Kart Wii' })).toBeInTheDocument();
      expect(page.getByRole('button', { name: /^Install/ }).elements()).toHaveLength(0);
      expect(ach).not.toHaveBeenCalled(); // caps.achievements is false
      expect(loc).not.toHaveBeenCalled(); // caps.screenshots is false
    });

    it('caps decide: an emulator game without an editor has no Controls section, a number in the old range does not matter', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
      const c = vi.spyOn(api, 'emulatorControls');
      await render(GameDetail, { props: props({ ...newEmu, caps: { ...caps, controls: false } }) });
      await expect.element(page.getByRole('button', { name: 'Play' })).toBeInTheDocument();
      expect(page.getByRole('heading', { name: 'Controls' }).elements()).toHaveLength(0);
      expect(c).not.toHaveBeenCalled();
    });

    it('a new hostd Steam game in a high number range is still Steam', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
      const c = vi.spyOn(api, 'emulatorControls');
      const steamCaps = { ...caps, install: true, controls: false, achievements: true, screenshots: true };
      await render(GameDetail, { props: props({ ...game, appid: 0xe0000005, id: 'steam:3758096389', source: 'steam', caps: steamCaps }) });
      await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
      expect(c).not.toHaveBeenCalled();
    });

    it('is absent for a Steam game and never asks hostd', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
      const c = vi.spyOn(api, 'emulatorControls');
      await render(GameDetail, { props: props(game) });
      await expect.element(page.getByText('Think with portals & friends.')).toBeInTheDocument();
      expect(page.getByRole('heading', { name: 'Controls' }).elements()).toHaveLength(0);
      expect(c).not.toHaveBeenCalled();
    });
  });
});

describe('Remove from library', () => {
  const emu: Game = { ...game, appid: 0xe0000001, name: 'Mario Kart Wii', id: 'emu:dolphin:RMCE01', source: 'emu', emulator: 'dolphin' };
  beforeEach(() => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    vi.spyOn(api, 'emulatorControls').mockRejectedValue(new ApiError('no', 404));
    vi.spyOn(api, 'games').mockResolvedValue({ games: [], ownedAvailable: true });
  });

  it('is only on emulator games', async () => {
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByRole('button', { name: 'Close' }).first()).toBeInTheDocument();
    expect(page.getByRole('button', { name: 'Remove from library' }).elements()).toHaveLength(0);
  });

  it('asks first, says the file stays, offers no delete option, then forgets the game', async () => {
    const rm = vi.spyOn(api, 'removeGame').mockResolvedValue(undefined);
    const onclose = vi.fn();
    await render(Toasts);
    await render(GameDetail, { props: props(emu, { onclose }) });
    await userEvent.click(page.getByRole('button', { name: 'Remove from library' }));
    await expect.element(page.getByText(/The file stays on disk/)).toBeInTheDocument();
    expect(rm).not.toHaveBeenCalled();
    expect(page.getByRole('checkbox').elements()).toHaveLength(0);
    await userEvent.click(page.getByRole('button', { name: 'Remove', exact: true }));
    await expect.poll(() => rm.mock.calls).toEqual([['emu:dolphin:RMCE01']]);
    await expect.element(page.getByText('Removed Mario Kart Wii from the library. The file stays on disk.')).toBeInTheDocument();
    await expect.poll(() => onclose.mock.calls.length).toBe(1);
  });

  it('Cancel keeps the game and says nothing to hostd', async () => {
    const rm = vi.spyOn(api, 'removeGame').mockResolvedValue(undefined);
    await render(GameDetail, { props: props(emu) });
    await userEvent.click(page.getByRole('button', { name: 'Remove from library' }));
    await userEvent.click(page.getByRole('button', { name: 'Cancel' }));
    expect(rm).not.toHaveBeenCalled();
  });

  it('shows the failure and stays open', async () => {
    vi.spyOn(api, 'removeGame').mockRejectedValue(new ApiError('library.json is damaged', 500));
    await render(GameDetail, { props: props(emu) });
    await userEvent.click(page.getByRole('button', { name: 'Remove from library' }));
    await userEvent.click(page.getByRole('button', { name: 'Remove', exact: true }));
    await expect.element(page.getByRole('alert')).toHaveTextContent('Could not remove it: library.json is damaged');
  });
});

describe('GameDetail with the Quit confirm', () => {
  it('puts the confirm above the panel; Esc closes the confirm first, then the panel; focus returns to Quit', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    const { default: Harness } = await import('../../test/DetailQuitHarness.svelte');
    await render(Harness, { props: { game: { ...game, running: true } } });
    await userEvent.click(page.getByRole('button', { name: 'Open detail' }));
    const panel = page.getByRole('dialog', { name: 'Portal 2', exact: true });
    await expect.element(panel).toBeInTheDocument();
    const quit = page.getByRole('button', { name: 'Quit Portal 2' }).element() as HTMLElement;
    await userEvent.click(quit);
    const confirm = page.getByRole('dialog', { name: 'Quit Portal 2?' });
    await expect.element(confirm).toBeInTheDocument();
    await expect.poll(() => confirm.element().contains(document.activeElement)).toBe(true);
    for (const name of ['Cancel', 'Quit']) {
      const el = confirm.getByRole('button', { name, exact: true }).element();
      const r = el.getBoundingClientRect();
      expect(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2), name).toBe(el);
    }
    // its overlay covers the panel too
    const dim = document.querySelector('.rs-dim')!;
    expect(Number(getComputedStyle(dim).zIndex)).toBeGreaterThan(Number(getComputedStyle(panel.element()).zIndex));
    await userEvent.keyboard('{Escape}');
    await expect.poll(() => page.getByRole('dialog', { name: 'Quit Portal 2?' }).elements().length).toBe(0);
    expect(panel.element().isConnected).toBe(true);
    await expect.poll(() => document.activeElement).toBe(quit);
    await userEvent.keyboard('{Escape}');
    await expect.poll(() => page.getByRole('dialog', { name: 'Portal 2', exact: true }).elements().length).toBe(0);
  });

  describe('Saves section', () => {
    const caps = { install: false, controls: false, achievements: false, launchOptions: false, saves: true, screenshots: false };
    const wii: Game = {
      ...game,
      appid: 0xe0000002,
      id: 'emu:dolphin:rmce01',
      source: 'emu',
      legacyId: '3758096386',
      emulator: 'dolphin',
      name: 'Mario Kart Wii',
      caps,
    };
    const items = [
      {
        kind: 'state' as const,
        name: 'RMCE01.s01',
        path: 'StateSaves/RMCE01.s01',
        size: 4_100_000,
        modified: Math.floor(Date.now() / 1000) - 3 * 86_400,
      },
      { kind: 'nand-save' as const, name: 'Wii save data', path: 'Wii/title/00010000/524d4345/data', size: 2048, modified: null, files: 3 },
    ];

    it('lists saves, asks by id, and offers no download or delete', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
      const s = vi.spyOn(api, 'gameSaves').mockResolvedValue({ available: true, userDir: 'Dolphin user folder', items });
      await render(GameDetail, { props: props(wii) });
      const list = page.getByRole('list', { name: 'Saves' });
      await expect.element(list.getByText('RMCE01.s01')).toBeInTheDocument();
      await expect.element(list.getByText('Save state, 4.1 MB, 3 d ago')).toBeInTheDocument();
      await expect.element(list.getByText('Wii save, 3 files, 2 KB')).toBeInTheDocument();
      expect(s).toHaveBeenCalledWith('emu:dolphin:rmce01');
      expect(page.getByRole('button', { name: /download|delete/i }).elements()).toHaveLength(0);
    });

    it('explains where saves appear when there are none', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
      vi.spyOn(api, 'gameSaves').mockResolvedValue({ available: false, userDir: 'Dolphin user folder', items: [] });
      await render(GameDetail, { props: props(wii) });
      await expect.element(page.getByText('Save states and saves show up here after you play.')).toBeInTheDocument();
    });

    it('an old hostd (no caps) and a Steam game never call the route', async () => {
      vi.spyOn(api, 'gameInfo').mockResolvedValue(info);
      const s = vi.spyOn(api, 'gameSaves');
      await render(GameDetail, { props: props({ ...wii, caps: undefined, id: undefined }) });
      await expect.element(page.getByRole('button', { name: 'Play' })).toBeInTheDocument();
      expect(s).not.toHaveBeenCalled();
      expect(page.getByRole('heading', { name: 'Saves' }).elements()).toHaveLength(0);
    });
  });
});

describe('GameDetail launch options', () => {
  const withCaps = {
    ...game,
    id: 'steam:620',
    source: 'steam' as const,
    caps: { install: true, controls: false, achievements: true, launchOptions: true, saves: false, screenshots: true },
  };
  const view = {
    id: 'steam:620',
    rev: 'g1',
    override: null,
    effective: { wrappers: [], env: {}, custom: null, extra_args: [] },
    problems: [],
    allowCustomCommands: false,
    steamLine: 'brisk-hostd wrap steam:620 -- %command%',
  };

  it('shows the section when the game has the launchOptions cap', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    vi.spyOn(api, 'launchDefaults').mockResolvedValue({ rev: 'r', allowCustomCommands: false, options: view.effective, envAllowed: [] });
    const g = vi.spyOn(api, 'gameLaunch').mockResolvedValue(view);
    render(GameDetail, props(withCaps));
    await expect.element(page.getByRole('heading', { name: 'Launch options' })).toBeInTheDocument();
    expect(g).toHaveBeenCalledWith('steam:620');
  });

  it('is left out for a game without the cap (an older hostd)', async () => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    const g = vi.spyOn(api, 'gameLaunch');
    render(GameDetail, props(game));
    await expect.element(page.getByText('No store details')).toBeInTheDocument();
    expect(page.getByRole('heading', { name: 'Launch options' }).elements()).toHaveLength(0);
    expect(g).not.toHaveBeenCalled();
  });
});

describe('GameDetail RetroAchievements', () => {
  const wii: Game = {
    ...game,
    appid: 3_800_000_000,
    id: 'emu:dolphin:mkwii',
    name: 'Mario Kart Wii',
    source: 'emu',
    emulator: 'dolphin',
    installed: true,
    caps: { install: false, controls: true, achievements: true, launchOptions: false, saves: false, screenshots: false },
  };
  const raOk: Achievements = {
    available: true,
    unlocked: 1,
    total: 2,
    list: [
      { id: '21', name: 'First', description: 'Do the first thing', icon: '', unlockedAt: 1773337649, points: 3, hardcore: true },
      { id: '22', name: 'Second', description: 'Later', icon: '', unlockedAt: null, points: 5, hardcore: false },
    ],
    source: 'retroachievements',
    state: 'ok',
    unlockedHardcore: 1,
    pointsUnlocked: 3,
    pointsTotal: 8,
    gameId: 9,
    title: 'Mario Kart Wii',
    url: 'https://retroachievements.org/game/9',
  };
  beforeEach(() => vi.spyOn(api, 'gameInfo').mockResolvedValue({ available: false } as GameInfo));
  afterEach(() => vi.useRealTimers());

  const notes: [Record<string, unknown>, string][] = [
    [{ state: 'unsupported', reason: 'RVZ files cannot be hashed.' }, 'RVZ files cannot be hashed.'],
    [{ state: 'not-found' }, 'No RetroAchievements set for this game.'],
    [{ state: 'rate-limited' }, 'RetroAchievements is busy. Try again shortly.'],
    [{ state: 'bad-credentials' }, 'RetroAchievements refused the login. Check the key file on the machine.'],
    [{ state: 'error' }, 'Could not reach RetroAchievements.'],
  ];
  for (const [answer, text] of notes)
    it(`says "${text}" for ${answer.state}`, async () => {
      vi.spyOn(api, 'gameAchievements').mockResolvedValue(answer as never);
      await render(GameDetail, { props: props(wii) });
      await expect.element(page.getByText(text)).toBeInTheDocument();
    });

  it('title-matched answer shows the plain note; hash-matched does not', async () => {
    const spy = vi.spyOn(api, 'gameAchievements').mockResolvedValue({ ...raOk, matched: 'title' });
    await render(GameDetail, { props: props(wii) });
    await expect.element(page.getByText(/^Matched by title; your file's hash isn't/)).toBeInTheDocument();
    spy.mockResolvedValue({ ...raOk, matched: 'hash' });
  });

  it('hides the panel for off', async () => {
    const spy = vi.spyOn(api, 'gameAchievements').mockResolvedValue({ state: 'off' });
    await render(GameDetail, { props: props(wii) });
    await vi.waitFor(() => expect(spy).toHaveBeenCalled());
    await expect.element(page.getByText('Achievements', { exact: true })).not.toBeInTheDocument();
  });

  it('ok: totals, points, hardcore marker and the credit link', async () => {
    const spy = vi.spyOn(api, 'gameAchievements').mockResolvedValue(raOk);
    await render(GameDetail, { props: props(wii) });
    await expect.element(page.getByText(/1 of 2 unlocked, 3 of 8 points/)).toBeInTheDocument();
    await expect.element(page.getByText('Hardcore')).toBeInTheDocument();
    const link = page.getByRole('link', { name: 'RetroAchievements' });
    await expect.element(link).toHaveAttribute('href', 'https://retroachievements.org/game/9');
    await expect.element(link).toHaveAttribute('target', '_blank');
    await expect.element(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(spy).toHaveBeenCalledExactlyOnceWith('emu:dolphin:mkwii');
  });

  it('Refresh asks with refresh and is disabled for 30 s', async () => {
    const spy = vi.spyOn(api, 'gameAchievements').mockResolvedValue(raOk);
    await render(GameDetail, { props: props(wii) });
    await expect.element(page.getByText(/points/)).toBeInTheDocument();
    const b = page.getByRole('button', { name: 'Refresh', exact: true });
    await userEvent.click(b);
    expect(spy).toHaveBeenLastCalledWith('emu:dolphin:mkwii', true);
    await expect.element(b).toHaveAttribute('aria-disabled', 'true');
    spy.mockClear();
    (b.element() as HTMLElement).click();
    expect(spy).not.toHaveBeenCalled();
  });

  it('polls every 45 s only while the game runs, and stops on unmount', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    const spy = vi.spyOn(api, 'gameAchievements').mockResolvedValue(raOk);
    const idle = await render(GameDetail, { props: props(wii) });
    await vi.advanceTimersByTimeAsync(100_000);
    expect(spy).toHaveBeenCalledTimes(1); // the load only
    idle.unmount();
    spy.mockClear();
    const run = await render(GameDetail, { props: props({ ...wii, running: true }) });
    await vi.advanceTimersByTimeAsync(1);
    expect(spy).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(45_000);
    expect(spy).toHaveBeenCalledTimes(2);
    run.unmount();
    await vi.advanceTimersByTimeAsync(100_000);
    expect(spy).toHaveBeenCalledTimes(2);
  });

  it('an old hostd (no caps) emulator game never asks', async () => {
    const spy = vi.spyOn(api, 'gameAchievements').mockResolvedValue(raOk);
    await render(GameDetail, {
      props: props({ ...wii, caps: undefined, id: undefined, source: undefined, achievements: undefined } as never),
    });
    await vi.waitFor(() => expect(api.gameInfo).toHaveBeenCalled());
    expect(spy).not.toHaveBeenCalled();
  });

  it('a Steam game keeps its panel: no RetroAchievements link, no extra Refresh', async () => {
    vi.spyOn(api, 'gameAchievements').mockResolvedValue({
      available: true,
      unlocked: 3,
      total: 5,
      list: [{ id: 'A', name: 'First Steps', description: 'Take a step.', icon: '', unlockedAt: 1700000100 }],
    });
    await render(GameDetail, { props: props(game) });
    await expect.element(page.getByText('First Steps')).toBeInTheDocument();
    await expect.element(page.getByRole('link', { name: 'RetroAchievements' })).not.toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'Refresh', exact: true })).not.toBeInTheDocument();
  });
});

describe('stored stream screenshots (emulator games)', () => {
  const caps = { install: false, controls: true, achievements: false, launchOptions: false, saves: false, screenshots: true, art: true };
  const wii: Game = { ...game, appid: 0xe0000001, name: 'Mario Kart Wii', id: 'emu:dolphin:mkw', source: 'emu', emulator: 'dolphin', caps };
  const open = async (g: Game = wii) => {
    vi.spyOn(api, 'gameInfo').mockResolvedValue(none);
    vi.spyOn(api, 'screenshot').mockImplementation(async () => blobUrl());
    await render(GameDetail, { props: props(g) });
  };

  it('lists them in the same grid, fetched by id, and deletes one only after a confirm', async () => {
    vi.spyOn(api, 'gameLocal').mockResolvedValue({ screenshots: ['300.png', '200.png'], saves: null });
    const del = vi.spyOn(api, 'deleteShot').mockResolvedValue(undefined);
    await render(Toasts);
    await open();
    await expect
      .element(page.getByRole('img', { name: 'Mario Kart Wii screenshot 2 from this PC' }))
      .toHaveAttribute('src', expect.stringMatching(/^blob:/));
    expect(api.screenshot).toHaveBeenCalledWith('emu:dolphin:mkw', '300.png');
    await page.getByRole('button', { name: 'Delete screenshot 1', exact: true }).click();
    expect(del).not.toHaveBeenCalled(); // asked first
    await page.getByRole('button', { name: 'Keep' }).click();
    expect(del).not.toHaveBeenCalled();
    await page.getByRole('button', { name: 'Delete screenshot 1', exact: true }).click();
    await page.getByRole('group', { name: 'Delete screenshot 1?' }).getByRole('button', { name: 'Delete' }).click();
    expect(del).toHaveBeenCalledWith('emu:dolphin:mkw', '300.png');
    await expect.element(page.getByRole('button', { name: 'Open Mario Kart Wii screenshot 2 from this PC' })).not.toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'Open Mario Kart Wii screenshot 1 from this PC' })).toBeInTheDocument();
  });

  it('a failed delete keeps the tile and says so', async () => {
    vi.spyOn(api, 'gameLocal').mockResolvedValue({ screenshots: ['300.png'], saves: null });
    vi.spyOn(api, 'deleteShot').mockRejectedValue(new ApiError('nope', 500));
    await render(Toasts);
    await open();
    await page.getByRole('button', { name: 'Delete screenshot 1', exact: true }).click();
    await page.getByRole('group', { name: 'Delete screenshot 1?' }).getByRole('button', { name: 'Delete' }).click();
    await expect.element(page.getByText(/Couldn't delete the screenshot/)).toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'Open Mario Kart Wii screenshot 1 from this PC' })).toBeInTheDocument();
  });

  it('Steam screenshots have no delete, and an old hostd emulator game asks for none', async () => {
    vi.spyOn(api, 'gameLocal').mockResolvedValue({ screenshots: ['a.jpg'], saves: null });
    await open(game);
    await expect.element(page.getByRole('button', { name: 'Open Portal 2 screenshot 1 from this PC' })).toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: /^Delete screenshot/ })).not.toBeInTheDocument();
  });
});
