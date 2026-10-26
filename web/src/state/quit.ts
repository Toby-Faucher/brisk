import { get, writable } from 'svelte/store';
import type { Game } from './api';
import type { CommandItem } from '../shell/registry';

export type QuitState = 'closed' | 'still-running';
export type ToastOpts = { error?: boolean; ms?: number; action?: { label: string; run: () => void } };
export type QuitDeps = {
  quit: (appid: number, force: boolean) => Promise<{ state: QuitState }>;
  refresh: () => Promise<unknown>;
  toast: (text: string, opts?: ToastOpts) => void;
};

/** The confirm on screen: which game, and whether it is the forced retry. */
export type Ask = { game: Game; force: boolean };

/** Quit a running game: confirm, busy while hostd works (up to ~25 s), then a toast and a games refresh. */
export function quitFlow(deps: QuitDeps) {
  const ask = writable<Ask | null>(null);
  const busy = writable<ReadonlySet<number>>(new Set());
  const mark = (id: number, on: boolean) =>
    busy.update((s) => {
      const n = new Set(s);
      if (on) n.add(id);
      else n.delete(id);
      return n;
    });

  function request(game: Game, force = false) {
    if (!get(busy).has(game.appid)) ask.set({ game, force });
  }

  async function confirm() {
    const a = get(ask);
    ask.set(null);
    if (!a || get(busy).has(a.game.appid)) return;
    const { game, force } = a;
    mark(game.appid, true);
    try {
      const r = await deps.quit(game.appid, force);
      if (r.state === 'closed') deps.toast(`Closed ${game.name}`);
      else deps.toast(`Still running: ${game.name} (try Force quit)`, { ms: 10_000, action: { label: 'Force quit', run: () => request(game, true) } });
    } catch (e) {
      deps.toast(`Could not quit ${game.name}: ${(e as Error).message}`, { error: true });
    } finally {
      mark(game.appid, false);
      await deps.refresh();
    }
  }

  return { ask, busy, request, confirm, cancel: () => ask.set(null) };
}

/** Command menu entries: one per game running on the host. */
export const quitItems = (games: Game[], request: (g: Game) => void): CommandItem[] =>
  games.filter((g) => g.running).map((g) => ({ id: `quit:${g.appid}`, label: `Quit ${g.name}`, icon: 'power', run: () => request(g) }));
