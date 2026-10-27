import type { Game } from '../state/api';
import type { Notes } from '../state/notes';
import { filterByTag, pickRandom, searchGames, SMART_TAGS } from '../modules/library/library';
import type { CommandItem } from './registry';

const openGame = (appid: number) => `/library?open=${appid}`;
const backlog = (games: Game[]) => filterByTag(games, { tags: {}, games: {} }, 'auto-backlog');

/** "Smart tag: <name> (n)" only while a game matches, then "Tag: <yours>" for every tag you made. Both open the Library filtered. */
export function tagEntries(notes: Notes, games: Game[], go: (path: string) => void): CommandItem[] {
  const auto = SMART_TAGS.map((t) => ({ t, n: games.filter(t.test).length }))
    .filter((x) => x.n > 0)
    .map(({ t, n }) => ({ id: `tag:${t.id}`, label: `Smart tag: ${t.name} (${n})`, tag: t, run: () => go(`/library?tag=${t.id}`) }));
  const mine = Object.entries(notes.tags).map(([id, t]) => ({ id: `tag:${id}`, label: `Tag: ${t.name}`, tag: t, run: () => go(`/library?tag=${id}`) }));
  return [...auto, ...mine];
}

/** Opens a random Backlog game; null when the Backlog is empty. The pick happens on run, so each choice is fresh. */
export function surpriseEntry(games: Game[], go: (path: string) => void, rnd: () => number = Math.random): CommandItem | null {
  if (!backlog(games).length) return null;
  return { id: 'cmd:surprise', label: 'Surprise me', hint: 'Backlog', icon: 'search', run: () => go(openGame(pickRandom(backlog(games), rnd)!.appid)) };
}

/** "Open <game>" for the best name matches (Library's folded search); none for an empty query. */
export function openEntries(games: Game[], query: string, go: (path: string) => void, cap = 8): CommandItem[] {
  if (!query.trim()) return [];
  return searchGames(games, query).slice(0, cap).map((g) => ({ id: `open:${g.appid}`, label: `Open ${g.name}`, icon: 'play', run: () => go(openGame(g.appid)) }));
}
