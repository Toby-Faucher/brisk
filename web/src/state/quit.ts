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
  /** Games whose last quit answered still-running: Ctrl+K offers Force quit for them. */
  const stuck = writable<ReadonlySet<number>>(new Set());
  const mark = (store: typeof busy, id: number, on: boolean) =>
    store.update((s) => {
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
    mark(busy, game.appid, true);
    try {
      const r = await deps.quit(game.appid, force);
      mark(stuck, game.appid, r.state === 'still-running');
      if (r.state === 'closed') deps.toast(`Closed ${game.name}`);
      else deps.toast(`Still running: ${game.name} (try Force quit)`, { ms: 30_000, action: { label: 'Force quit', run: () => request(game, true) } });
    } catch (e) {
      const err = e as Error & { status?: number };
      // Another tab or button started this quit: its answer is still coming.
      if (err.status === 409 && err.message === 'already quitting') deps.toast(`Already closing ${game.name}…`);
      else deps.toast(`Could not quit ${game.name}: ${err.message}`, { error: true });
    } finally {
      mark(busy, game.appid, false);
      await deps.refresh();
    }
  }

  return { ask, busy, stuck, request, confirm, cancel: () => ask.set(null) };
}

/** Command menu entries for games running on the host: Quit, plus Force quit after a quit left one running. */
export const quitItems = (games: Game[], stuck: ReadonlySet<number>, request: (g: Game, force: boolean) => void): CommandItem[] =>
  games
    .filter((g) => g.running)
    .flatMap((g) => [
      { id: `quit:${g.appid}`, label: `Quit ${g.name}`, icon: 'power', run: () => request(g, false) },
      ...(stuck.has(g.appid) ? [{ id: `forcequit:${g.appid}`, label: `Force quit ${g.name}`, icon: 'power', run: () => request(g, true) }] : []),
    ]);
