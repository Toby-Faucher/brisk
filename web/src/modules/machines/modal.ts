import type { Game } from '../../state/api';
import { fmtSize } from '../library/library';
import { fmtDate } from '../../ui/format';

export type SortKey = 'name' | 'size' | 'played' | 'playtime';
export const SORTS: { key: SortKey; label: string }[] = [
  { key: 'name', label: 'Name' },
  { key: 'size', label: 'Size' },
  { key: 'played', label: 'Last played' },
  { key: 'playtime', label: 'Playtime' },
];
export type Layout = 'wide' | 'medium' | 'narrow';

const KEY = 'brisk.machineGamesSort';
const byName = (a: Game, b: Game) => a.name.localeCompare(b.name);

/** Widths where three columns, then two (pane 3 slides over pane 2), then one pane at a time stop fitting. */
export const layoutFor = (w: number): Layout => (w >= 1100 ? 'wide' : w >= 700 ? 'medium' : 'narrow');

export const isSortKey = (v: unknown): v is SortKey => SORTS.some((s) => s.key === v);

/** Installed games matching `q` (case-insensitive substring), best first for `sort`; ties fall back to name. Never mutates. */
export function listGames(games: Game[], q: string, sort: SortKey): Game[] {
  const n = q.trim().toLowerCase();
  const num: Record<Exclude<SortKey, 'name'>, (g: Game) => number> = {
    size: (g) => g.sizeBytes ?? 0,
    played: (g) => g.lastPlayed ?? 0,
    playtime: (g) => g.playtimeMin,
  };
  return games
    .filter((g) => g.installed && g.name.toLowerCase().includes(n))
    .sort((a, b) => (sort === 'name' ? 0 : num[sort](b) - num[sort](a)) || byName(a, b));
}

/** `12 games · 340 GB`; the size part is left out when no game reports one. */
export function gamesHeader(games: Game[]): string {
  const total = games.reduce((s, g) => s + (g.sizeBytes ?? 0), 0);
  const size = fmtSize(total);
  return `${games.length} ${games.length === 1 ? 'game' : 'games'}${size ? ` · ${size}` : ''}`;
}

/** Last played (unix seconds) as a short date, or "Never". */
export const fmtPlayed = (s: number | null) => (s ? fmtDate(s) : 'Never');

export const fmtPlaytime = (min: number) =>
  min < 1 ? 'Not played yet' : min < 60 ? `${min} min` : `${(min / 60).toFixed(min < 600 ? 1 : 0)} h`;

/** The remembered sort, or Name when storage is blocked or holds nonsense. */
export function loadSort(): SortKey {
  try {
    const v = sessionStorage.getItem(KEY);
    return isSortKey(v) ? v : 'name';
  } catch {
    return 'name';
  }
}
export function saveSort(k: SortKey) {
  try {
    sessionStorage.setItem(KEY, k);
  } catch {}
}

/** Index after an arrow/Home/End key in a list of `n`, or null for other keys. */
export function stepIndex(i: number, key: string, n: number): number | null {
  if (!n) return null;
  if (key === 'ArrowDown') return Math.min(n - 1, i + 1);
  if (key === 'ArrowUp') return Math.max(0, i - 1);
  if (key === 'Home') return 0;
  return key === 'End' ? n - 1 : null;
}
