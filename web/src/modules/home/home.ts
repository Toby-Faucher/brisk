import type { Game } from '../../state/api';
import type { Tag } from '../../state/notes';

/** The installed game running on the host, else the one played most recently (lastPlayed is unix seconds), or null. */
export function pickResume(games: Game[]): Game | null {
  let best: Game | null = null;
  const better = (a: Game, b: Game) => (a.running !== b.running ? a.running : a.lastPlayed! > b.lastPlayed!);
  for (const g of games) if (g.installed && g.lastPlayed != null && (!best || better(g, best))) best = g;
  return best;
}

/** The saved Home order laid over the cards there are now: saved ids in their order, new cards appended, gone ones dropped. */
export function reconcileOrder(saved: readonly string[], current: readonly string[]): string[] {
  const have = new Set(current);
  const kept = [...new Set(saved)].filter((id) => have.has(id));
  const seen = new Set(kept);
  return [...kept, ...current.filter((id) => !seen.has(id))];
}

/**
 * The order to store after a drop. While the notes are not loaded no tag card is shown, so `next` lacks them:
 * keep the saved `tag-*` ids, or the drag would erase their positions. Once loaded, a tag that is gone stays gone.
 */
export function orderToStore(next: readonly string[], saved: readonly string[], notesLoaded: boolean): string[] {
  if (notesLoaded) return [...next];
  return [...next, ...saved.filter((id) => id.startsWith('tag-') && !next.includes(id))];
}

/** A copy of `list` with the item at `from` moved to `to` (clamped); an out-of-range `from` changes nothing. */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  const a = [...list];
  if (from < 0 || from >= a.length) return a;
  const [x] = a.splice(from, 1);
  a.splice(Math.max(0, Math.min(a.length, to)), 0, x);
  return a;
}

/** Home card ids of the pinned tags, by name (case-insensitive) then id: `tag-` + the tag id, so at most 20 chars of [a-z0-9-]. */
export const tagCardIds = (tags: Record<string, Tag>): string[] =>
  Object.entries(tags)
    .filter(([, t]) => t.pinned)
    .sort(([ia, a], [ib, b]) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()) || (ia < ib ? -1 : 1))
    .map(([id]) => `tag-${id}`);
