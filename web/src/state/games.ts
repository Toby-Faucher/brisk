import { writable } from 'svelte/store';
import { api, type Game } from './api';

/** Shared by Home, Library and the command menu; null until the first fetch succeeds. */
export const games = writable<{ games: Game[]; ownedAvailable: boolean } | null>(null);

/** True when the last fetch failed (hostd unreachable): lets pages tell loading from unavailable. */
export const gamesFailed = writable(false);

let fetchedAt = -Infinity;

export async function refreshGames() {
  fetchedAt = Date.now();
  const g = await api.games().catch(() => null);
  gamesFailed.set(!g);
  if (g) games.set(g);
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
