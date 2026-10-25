import type { Game } from '../../state/api';
import { pickResume } from '../home/home';

export type Filter = 'all' | 'installed' | 'not-installed' | 'recent';

const RECENT_S = 30 * 86_400;

/** `now` is unix ms; Game.lastPlayed is unix seconds. */
export function filterGames(games: Game[], f: Filter, now = Date.now()): Game[] {
  if (f === 'installed') return games.filter((g) => g.installed);
  if (f === 'not-installed') return games.filter((g) => !g.installed);
  if (f === 'recent') return games.filter((g) => g.lastPlayed != null && now / 1000 - g.lastPlayed <= RECENT_S);
  return games;
}

/** Lowercase, accents dropped, punctuation as single spaces: "Pokémon: Legends" -> "pokemon legends". */
const fold = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim();

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
export function shelves(games: Game[]): { installed: Game[]; notInstalled: Game[]; hero: Game | null } {
  return { installed: games.filter((g) => g.installed), notInstalled: games.filter((g) => !g.installed), hero: pickResume(games) };
}

const OVERSCAN = 2;
const MAX_MOUNTED = 20;

/** Card indexes [start, end) to mount for a horizontal shelf; `stride` is card width plus gap. */
export function visibleRange(scrollLeft: number, width: number, stride: number, total: number): { start: number; end: number } {
  const start = Math.max(0, Math.min(total, Math.floor(scrollLeft / stride) - OVERSCAN));
  const end = Math.min(total, Math.ceil((scrollLeft + width) / stride) + OVERSCAN, start + MAX_MOUNTED);
  return { start, end };
}
