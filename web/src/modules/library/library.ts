import type { Game, GameLocal } from '../../state/api';
import { fmtAgo, pickResume } from '../home/home';
import type { Notes } from '../../state/notes';

export type Filter = 'all' | 'installed' | 'not-installed' | 'recent';

export type Sort = 'default' | 'az' | 'most-played' | 'last-played';

const RECENT_S = 30 * 86_400;

/** `now` is unix ms; Game.lastPlayed is unix seconds. */
export function filterGames(games: Game[], f: Filter, now = Date.now()): Game[] {
  if (f === 'installed') return games.filter((g) => g.installed);
  if (f === 'not-installed') return games.filter((g) => !g.installed);
  if (f === 'recent') return games.filter((g) => g.lastPlayed != null && now / 1000 - g.lastPlayed <= RECENT_S);
  return games;
}

/** null: all games; else the games carrying tag `tagId` (while it exists). Keeps the order. */
export function filterByTag(games: Game[], notes: Notes, tagId: string | null): Game[] {
  if (tagId === null) return games;
  if (!notes.tags[tagId]) return [];
  return games.filter((g) => notes.games[g.appid]?.tags.includes(tagId));
}

/** 'default' keeps the incoming order (search relevance, then most played); the others are stable sorts of a copy. */
export function sortGames(games: Game[], s: Sort): Game[] {
  if (s === 'az') return [...games].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }));
  if (s === 'most-played') return [...games].sort((a, b) => b.playtimeMin - a.playtimeMin);
  if (s === 'last-played') return [...games].sort((a, b) => (b.lastPlayed ?? -1) - (a.lastPlayed ?? -1)); // never played last
  return games;
}

/** Lowercase, accents dropped, punctuation as single spaces: "Pokémon: Legends" -> "pokemon legends". */
export const fold = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim();

/** Matches on a word start rank before plain substrings; ties (and an empty query) go most played first. */
export function searchGames(games: Game[], q: string): Game[] {
  const fq = fold(q);
  const ranked: [number, Game][] = [];
  for (const g of games) {
    const n = ' ' + fold(g.name);
    if (n.includes(' ' + fq)) ranked.push([0, g]);
    else if (n.includes(fq)) ranked.push([1, g]);
  }
  return ranked.sort((a, b) => a[0] - b[0] || b[1].playtimeMin - a[1].playtimeMin).map((r) => r[1]);
}

/** Splits keeping input order; the hero is the same game Home's Resume card offers. */
export function shelves(games: Game[]): { installed: Game[]; notInstalled: Game[]; fortnight: Game[]; hero: Game | null } {
  const fortnight = games.filter((g) => g.playtime2wkMin > 0).sort((a, b) => b.playtime2wkMin - a.playtime2wkMin);
  return { installed: games.filter((g) => g.installed), notInstalled: games.filter((g) => !g.installed), fortnight, hero: pickResume(games) };
}

const OVERSCAN = 2;
const MAX_MOUNTED = 20;

/** Card indexes [start, end) to mount for a horizontal shelf; `stride` is card width plus gap. */
export function visibleRange(scrollLeft: number, width: number, stride: number, total: number): { start: number; end: number } {
  const start = Math.max(0, Math.min(total, Math.floor(scrollLeft / stride) - OVERSCAN));
  const end = Math.min(total, Math.ceil((scrollLeft + width) / stride) + OVERSCAN, start + MAX_MOUNTED);
  return { start, end };
}

export const fmtFortnight = (min: number) => `${min < 60 ? `${min} min` : `${(min / 60).toFixed(1)} h`} this fortnight`;
export const fmtSize = (b: number | null) => (b ? `${(b / 1e9).toFixed(b < 1e10 ? 1 : 0)} GB` : null);
/** "Cloud saves: 14 files, 2.1 MB, synced 3 d ago"; null when there are no save files. `now` is unix ms. */
export function savesLine(s: GameLocal['saves'], now = Date.now()): string | null {
  if (!s?.files) return null;
  const b = s.bytes;
  const size = b < 1e6 ? `${Math.max(1, Math.round(b / 1e3))} KB` : b < 1e9 ? `${(b / 1e6).toFixed(1)} MB` : `${(b / 1e9).toFixed(1)} GB`;
  return `Cloud saves: ${s.files} ${s.files === 1 ? 'file' : 'files'}, ${size}${s.modified ? `, synced ${fmtAgo(s.modified * 1000, now)}` : ''}`;
}
export const installLabel = (g: Game) => (g.sizeBytes ? `Install ${fmtSize(g.sizeBytes)}` : 'Install');
/** A game still running on the host is resumed (streamed as is), not launched again. */
export const playLabel = (g: Game) => (g.running ? 'Resume' : 'Play');
export const badgeLabel = (g: Game) => (g.running ? 'Running' : g.installed ? 'Installed' : (fmtSize(g.sizeBytes) ?? 'Not installed'));

/** Every card opens the detail panel; Play and Install live there, so a click never starts or installs anything. */
export function cardClick(_g: Game): { action: 'open' } {
  return { action: 'open' };
}

/** Pixels to scroll a shelf sideways for a vertical wheel, or null to leave the event to the page (shelf at that edge, or nothing to scroll). */
export function wheelToX(dx: number, dy: number, scrollLeft: number, clientWidth: number, scrollWidth: number): number | null {
  if (Math.abs(dy) <= Math.abs(dx)) return null;
  if (dy < 0 ? scrollLeft <= 0 : scrollLeft + clientWidth >= scrollWidth - 1) return null;
  return dy;
}
