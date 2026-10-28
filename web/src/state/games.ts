import { derived, writable } from 'svelte/store';
import { api, type Game } from './api';
import { hub, selected } from './machines';

/** Shared by Home, Library and the command menu; null until the first fetch succeeds. */
export const games = writable<{ games: Game[]; ownedAvailable: boolean } | null>(null);

/** True when the last fetch failed (hostd unreachable): lets pages tell loading from unavailable. */
export const gamesFailed = writable(false);

let fetchedAt = -Infinity;
let gen = 0;

// Another machine was chosen: its games replace the old machine's list (a reply still in flight for the old one is dropped).
let machine: string | null = null;
derived([hub, selected], ([$h, $s]) => ($h ? $s : null)).subscribe((id) => {
  const switched = machine !== null && id !== null && id !== machine;
  machine = id;
  if (!switched) return;
  gen++;
  games.set(null);
  gamesFailed.set(false);
  void refreshGames();
});

/** `force` skips hostd's 6 h owned-games cache. Resolves true when the list arrived. */
export async function refreshGames({ force = false }: { force?: boolean } = {}): Promise<boolean> {
  fetchedAt = Date.now();
  const mine = gen;
  const g = await api.games(force || undefined).catch(() => null);
  if (mine !== gen) return false;
  gamesFailed.set(!g);
  if (g) games.set(g);
  return !!g;
}

const stale = () => Date.now() - fetchedAt >= 60_000;

/** Fetch if never fetched (or older than a minute), then again on window focus at most once a minute. */
export function watchGames(win: EventTarget = window): () => void {
  const onFocus = () => {
    if (stale()) void refreshGames();
  };
  onFocus();
  win.addEventListener('focus', onFocus);
  return () => win.removeEventListener('focus', onFocus);
}
