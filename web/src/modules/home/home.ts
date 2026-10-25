import type { Game } from '../../state/api';

export const DASH = '–';

/** The installed game played most recently (lastPlayed is unix seconds), or null. */
export function pickResume(games: Game[]): Game | null {
  let best: Game | null = null;
  for (const g of games) if (g.installed && g.lastPlayed != null && (!best || g.lastPlayed > best.lastPlayed!)) best = g;
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
