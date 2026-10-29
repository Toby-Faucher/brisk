import { errMsg } from './errMsg';
import { get, writable } from 'svelte/store';
import type { Game } from './api';
import { gameKey } from './gameId';
import type { CommandItem } from '../shell/registry';

export type QuitState = 'closed' | 'still-running';
/** `thumb`: an object URL shown small beside the text (decorative: the text says what happened). */
export type ToastOpts = { error?: boolean; ms?: number; thumb?: string; action?: { label: string; run: () => void } };
export type QuitDeps = {
  quit: (key: string, force: boolean) => Promise<{ state: QuitState }>;
  refresh: () => Promise<unknown>;
  toast: (text: string, opts?: ToastOpts) => void;
};

/** The confirm on screen: which game, and whether it is the forced retry. */
export type Ask = { game: Game; force: boolean };

/** Quit a running game: confirm, busy while hostd works (up to ~25 s), then a toast and a games refresh. */
export function quitFlow(deps: QuitDeps) {
  const ask = writable<Ask | null>(null);
  const busy = writable<ReadonlySet<string>>(new Set());
  /** Games whose last quit answered still-running: Ctrl+K offers Force quit for them. */
  const stuck = writable<ReadonlySet<string>>(new Set());
  const mark = (store: typeof busy, id: string, on: boolean) =>
    store.update((s) => {
      const n = new Set(s);
      if (on) n.add(id);
      else n.delete(id);
      return n;
    });

  function request(game: Game, force = false) {
    if (!get(busy).has(gameKey(game))) ask.set({ game, force });
  }

  async function confirm() {
    const a = get(ask);
    ask.set(null);
    if (!a || get(busy).has(gameKey(a.game))) return;
    const { game, force } = a;
    mark(busy, gameKey(game), true);
    try {
      const r = await deps.quit(gameKey(game), force);
      mark(stuck, gameKey(game), r.state === 'still-running');
      if (r.state === 'closed') deps.toast(`Closed ${game.name}`);
      else
        deps.toast(`Still running: ${game.name} (try Force quit)`, {
          ms: 30_000,
          action: { label: 'Force quit', run: () => request(game, true) },
        });
    } catch (e) {
      const err = e as Error & { status?: number };
      // Another tab or button started this quit: its answer is still coming.
      if (err.status === 409 && err.message === 'already quitting') deps.toast(`Already closing ${game.name}…`);
      else deps.toast(`Could not quit ${game.name}: ${errMsg(e)}`, { error: true });
    } finally {
      mark(busy, gameKey(game), false);
      await deps.refresh();
    }
  }

  return { ask, busy, stuck, request, confirm, cancel: () => ask.set(null) };
}

/** Command menu entries for games running on the host: Quit, plus Force quit after a quit left one running. */
export const quitItems = (games: Game[], stuck: ReadonlySet<string>, request: (g: Game, force: boolean) => void): CommandItem[] =>
  games
    .filter((g) => g.running)
    .flatMap((g) => [
      { id: `quit:${gameKey(g)}`, label: `Quit ${g.name}`, icon: 'power', run: () => request(g, false) },
      ...(stuck.has(gameKey(g))
        ? [{ id: `forcequit:${gameKey(g)}`, label: `Force quit ${g.name}`, icon: 'power', run: () => request(g, true) }]
        : []),
    ]);
