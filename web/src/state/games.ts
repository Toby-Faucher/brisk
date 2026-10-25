import { writable } from 'svelte/store';
import { api, type Game } from './api';

/** Shared by Library and the command menu; null until the first fetch succeeds. Task 8 adds refresh on focus. */
export const games = writable<{ games: Game[]; ownedAvailable: boolean } | null>(null);

export async function refreshGames() {
  const g = await api.games().catch(() => null);
  if (g) games.set(g);
}
