// RetroAchievements answers for emulator games. The route always answers 200 with a `state`; a Steam answer has none.
// Branch on `state`, never on the status. This file holds the pure parts: the mapping, the poll loop, the cooldown.
import type { Achievements, RaAnswer } from './api';

export type RaView = { ach: Achievements | null; note: string | null };

const NOTES: Record<string, string> = {
  'not-found': 'No RetroAchievements set for this game.',
  'rate-limited': 'RetroAchievements is busy. Try again shortly.',
  'bad-credentials': 'RetroAchievements refused the login. Check the key file on the machine.',
  error: 'Could not reach RetroAchievements.',
};

/** What to show for an answer: the achievements, a one-line note, or nothing (`off`). A Steam answer (no state) is shown as is. */
export function raView(a: Achievements | RaAnswer): RaView {
  const s: string = 'state' in a && a.state ? a.state : 'ok';
  if (s === 'ok') return { ach: a as Achievements, note: null };
  if (s === 'off') return { ach: null, note: null };
  if (s === 'unsupported') return { ach: null, note: (a as { reason?: string }).reason || 'This game file cannot be checked.' };
  return { ach: null, note: NOTES[s] ?? NOTES.error };
}

export const POLL_MS = 45_000;
export const BACKOFF_MAX_MS = 10 * 60_000;
export const REFRESH_COOLDOWN_MS = 30_000;

/** Calls `fetchOnce` every `every` ms, never two at once (the next wait starts when the call settles). A `rate-limited`
 *  answer doubles the wait (up to 10 min); anything else resets it. Returns the stop function. */
export function startPoll(fetchOnce: () => Promise<Achievements | RaAnswer | void>, every = POLL_MS): () => void {
  let stopped = false;
  let wait = every;
  let t: ReturnType<typeof setTimeout>;
  const tick = async () => {
    try {
      const v = await fetchOnce();
      wait = v && 'state' in v && v.state === 'rate-limited' ? Math.min(wait * 2, BACKOFF_MAX_MS) : every;
    } catch {
      wait = every; // a network failure keeps what is shown; try again on the normal beat
    }
    if (!stopped) t = setTimeout(tick, wait);
  };
  t = setTimeout(tick, wait);
  return () => {
    stopped = true;
    clearTimeout(t);
  };
}
