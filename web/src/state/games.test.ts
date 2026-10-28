import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('games', () => {
  it('games_store_refreshes_on_focus_not_more_than_once_a_minute', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const { watchGames } = await import('./games');
    (await import('./api')).setRoute(null);
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
    (await import('./api')).setRoute(null);
    const f = vi.fn(async (_: string) => new Response('{"games":[],"ownedAvailable":true}'));
    vi.stubGlobal('fetch', f);
    await refreshGames();
    await refreshGames({ force: true });
    expect(f.mock.calls.map((c) => c[0])).toEqual(['/api/games', '/api/games?refresh=1']);
  });
});

describe('games with a hub', () => {
  const mach = (id: string) => ({ id, name: id, state: 'ready', canPower: false, sunshine: null, host: null, url: '', rttMs: 1, vm: null });
  /** A hub whose /api/machines answer we hold back; records every other path. */
  async function hubSetup(machinesStatus = 200) {
    vi.resetModules();
    vi.stubGlobal('localStorage', { getItem: () => null, setItem() {}, removeItem() {} });
    vi.stubGlobal('sessionStorage', { getItem: () => null, setItem() {} });
    vi.stubGlobal('location', { href: 'http://localhost/' });
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    const paths: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (u: string) => {
        if (u === '/api/machines') {
          await held;
          return machinesStatus === 200
            ? new Response(JSON.stringify({ machines: [mach('a'), mach('b')] }))
            : new Response('{}', { status: machinesStatus });
        }
        if (u === '/api/prefs') return new Response('{"prefs":{}}');
        paths.push(u);
        return new Response('{"games":[],"ownedAvailable":true}');
      }),
    );
    const games = await import('./games');
    const machines = await import('./machines');
    return { games, machines, paths, release };
  }
  const settle = () => new Promise((r) => setTimeout(r, 20));

  it('never_asks_the_hub_itself_for_games_before_the_hub_is_probed', async () => {
    const { games, machines, paths, release } = await hubSetup();
    const stop = machines.startMachines();
    const done = games.refreshGames();
    await settle();
    expect(paths).toEqual([]); // held: not sent to the hub's own /api/games
    release();
    expect(await done).toBe(true);
    expect(paths).toEqual(['/api/m/a/games']);
    stop();
  });

  it('single_host_fetches_plain_api_games_once_the_probe_says_404', async () => {
    const { games, machines, paths, release } = await hubSetup(404);
    const stop = machines.startMachines();
    const done = games.refreshGames();
    release();
    expect(await done).toBe(true);
    expect(paths).toEqual(['/api/games']);
    stop();
  });

  it('switching_machine_drops_the_old_list_and_loads_the_new_one', async () => {
    const { games, machines, paths, release } = await hubSetup();
    const stop = machines.startMachines();
    release();
    await games.refreshGames();
    games.games.set({ games: [], ownedAvailable: false });
    machines.selected.set('b');
    await settle();
    expect(paths).toEqual(['/api/m/a/games', '/api/m/b/games']);
    expect(get(games.games)?.ownedAvailable).toBe(true);
    stop();
  });
});
