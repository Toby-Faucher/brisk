import type { Game } from '../../state/api';
import type { Tag } from '../../state/notes';

export const DASH = '–';

/** The installed game running on the host, else the one played most recently (lastPlayed is unix seconds), or null. */
export function pickResume(games: Game[]): Game | null {
  let best: Game | null = null;
  const better = (a: Game, b: Game) => (a.running !== b.running ? a.running : a.lastPlayed! > b.lastPlayed!);
  for (const g of games) if (g.installed && g.lastPlayed != null && (!best || better(g, best))) best = g;
  return best;
}

export const fmtTemp = (c: number | null) => (c == null ? DASH : `${Math.round(c)} °C`);

export function fmtUptime(s: number | null): string {
  if (s == null) return DASH;
  const h = Math.floor(s / 3600);
  if (h < 1) return `${Math.floor(s / 60)} min`;
  return h < 24 ? `${h} h` : `${Math.floor(h / 24)} d ${h % 24} h`;
}

/** `ms` and `now` are unix milliseconds. */
export function fmtAgo(ms: number | null, now = Date.now()): string {
  if (ms == null) return DASH;
  const m = Math.floor((now - ms) / 60_000);
  if (m < 1) return 'just now';
  if (m < 60) return `${m} min ago`;
  return m < 1440 ? `${Math.floor(m / 60)} h ago` : `${Math.floor(m / 1440)} d ago`;
}

export function fmtDuration(start: number, end: number | null): string {
  if (end == null) return 'live';
  const m = Math.round((end - start) / 60_000);
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, '0')}`;
}

/** The saved Home order laid over the cards there are now: saved ids in their order, new cards appended, gone ones dropped. */
export function reconcileOrder(saved: readonly string[], current: readonly string[]): string[] {
  const have = new Set(current);
  const kept = [...new Set(saved)].filter((id) => have.has(id));
  const seen = new Set(kept);
  return [...kept, ...current.filter((id) => !seen.has(id))];
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
