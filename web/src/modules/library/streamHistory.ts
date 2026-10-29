import type { Preset, SessionRec } from '../../state/api';
import { rowKey, sameKey } from '../../state/gameId';

/** Newest closed sessions looked at, how many of them must be bad, and the dropped/sent ratio that makes one bad. */
const RECENT_SESSIONS = 5;
const BAD_SESSIONS_NEEDED = 3;
const DROP_RATIO = 0.02;

const closed = (sessions: SessionRec[], key: string) => sessions.filter((s) => s.ended != null && sameKey(rowKey(s) ?? '', key));

export function gameHistory(
  sessions: SessionRec[],
  key: string,
): { count: number; totalMs: number; last: number | null; usualPreset: Preset | null } {
  const list = closed(sessions, key).sort((a, b) => a.started - b.started);
  const n = new Map<Preset, number>();
  for (const s of list) n.set(s.preset, (n.get(s.preset) ?? 0) + 1); // oldest first, so a tie below goes to the latest
  let usualPreset: Preset | null = null;
  let best = 0;
  for (const s of list) if ((n.get(s.preset) ?? 0) >= best) (best = n.get(s.preset)!), (usualPreset = s.preset);
  return {
    count: list.length,
    totalMs: list.reduce((t, s) => t + (s.ended! - s.started), 0),
    last: list.at(-1)?.started ?? null,
    usualPreset,
  };
}

const isBad = (s: SessionRec) => {
  const st = s.stats;
  return !!st && (st.gate === 'fail' || (!!st.sent && st.sent > 0 && (st.dropped ?? 0) / st.sent > DROP_RATIO));
};
const CHEAPER: Partial<Record<Preset, Preset>> = { crisp: 'balanced', balanced: 'fast' };

/** The newest closed sessions looked at (`of`), the preset of the newest (`from`) and how many sessions on that preset were bad. */
export function recentTrouble(sessions: SessionRec[], key: string): { from: Preset | null; bad: number; of: number } {
  const recent = closed(sessions, key)
    .sort((a, b) => b.started - a.started)
    .slice(0, RECENT_SESSIONS);
  const from = recent[0]?.preset ?? null;
  return { from, bad: recent.filter((s) => s.preset === from && isBad(s)).length, of: recent.length };
}

/** The next cheaper preset when the newest sessions at the newest session's preset kept going badly; never up. */
export function recommendPreset(sessions: SessionRec[], key: string): Preset | null {
  const { from, bad } = recentTrouble(sessions, key);
  return (from && bad >= BAD_SESSIONS_NEEDED && CHEAPER[from]) || null;
}

/** Total streamed time: minutes under an hour, else whole hours; rounds first so 59.6 min is "1 h", not "60 min". */
export function fmtTotal(ms: number): string {
  const m = Math.round(ms / 60_000);
  return m < 60 ? `${m} min` : `${Math.round(m / 60)} h`;
}
