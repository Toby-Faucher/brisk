import { afterEach, describe, expect, it, vi } from 'vitest';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('games', () => {
  it('games_store_refreshes_on_focus_not_more_than_once_a_minute', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const { watchGames } = await import('./games');
    const f = vi.fn(async () => new Response('{"games":[],"ownedAvailable":true}'));
    vi.stubGlobal('fetch', f);
    const win = new EventTarget();
    const focus = () => win.dispatchEvent(new Event('focus'));

    const stop = watchGames(win);
    expect(f).toHaveBeenCalledTimes(1); // first mount fetches
    focus();
    expect(f).toHaveBeenCalledTimes(1); // too soon
    await vi.advanceTimersByTimeAsync(61_000);
    focus();
    focus();
    expect(f).toHaveBeenCalledTimes(2);
    stop();

    const again = watchGames(win); // a remount within the minute does not refetch
    expect(f).toHaveBeenCalledTimes(2);
    again();
    await vi.advanceTimersByTimeAsync(61_000);
    focus();
    expect(f).toHaveBeenCalledTimes(2); // listener removed
  });

  it('refreshGames_force_adds_refresh_1_and_plain_does_not', async () => {
    vi.resetModules();
    const { refreshGames } = await import('./games');
    const f = vi.fn(async (_: string) => new Response('{"games":[],"ownedAvailable":true}'));
    vi.stubGlobal('fetch', f);
    await refreshGames();
    await refreshGames({ force: true });
    expect(f.mock.calls.map((c) => c[0])).toEqual(['/api/games', '/api/games?refresh=1']);
  });
});
