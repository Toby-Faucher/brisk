import type { Preset, SessionRec } from '../../state/api';

/** Newest closed sessions looked at, how many of them must be bad, and the dropped/sent ratio that makes one bad. */
export const RECENT_SESSIONS = 5;
export const BAD_SESSIONS_NEEDED = 3;
export const DROP_RATIO = 0.02;

const closed = (sessions: SessionRec[], appid: number) => sessions.filter((s) => s.appid === appid && s.ended != null);

export function gameHistory(sessions: SessionRec[], appid: number): { count: number; totalMs: number; last: number | null; usualPreset: Preset | null } {
  const list = closed(sessions, appid).sort((a, b) => a.started - b.started);
  const n = new Map<Preset, number>();
  for (const s of list) n.set(s.preset, (n.get(s.preset) ?? 0) + 1); // oldest first, so a tie below goes to the latest
  let usualPreset: Preset | null = null;
  let best = 0;
  for (const s of list) if ((n.get(s.preset) ?? 0) >= best) (best = n.get(s.preset)!, (usualPreset = s.preset));
  return { count: list.length, totalMs: list.reduce((t, s) => t + (s.ended! - s.started), 0), last: list.at(-1)?.started ?? null, usualPreset };
}

const isBad = (s: SessionRec) => {
  const st = s.stats;
  return !!st && (st.gate === 'fail' || (!!st.sent && st.sent > 0 && (st.dropped ?? 0) / st.sent > DROP_RATIO));
};
const CHEAPER: Partial<Record<Preset, Preset>> = { crisp: 'balanced', balanced: 'fast' };

/** The next cheaper preset when the newest sessions at the newest session's preset kept dropping frames; never up. */
export function recommendPreset(sessions: SessionRec[], appid: number): Preset | null {
  const recent = closed(sessions, appid).sort((a, b) => b.started - a.started).slice(0, RECENT_SESSIONS);
  const from = recent[0]?.preset;
  const to = from && CHEAPER[from];
  return to && recent.filter((s) => s.preset === from && isBad(s)).length >= BAD_SESSIONS_NEEDED ? to : null;
}

/** For the suggestion line: how many of the recent sessions were bad, and how many were looked at. */
export function badCount(sessions: SessionRec[], appid: number): { bad: number; of: number } {
  const recent = closed(sessions, appid).sort((a, b) => b.started - a.started).slice(0, RECENT_SESSIONS);
  return { bad: recent.filter((s) => s.preset === recent[0]?.preset && isBad(s)).length, of: recent.length };
}
