// A hub that held notes or prefs under an old emulator number cannot rekey them at start (it has no library). When a
// machine reports `legacyId` and `id` for such a game, ask the hub to move them once. See docs/plans/2026-10-28-stage1-api.md section 4.
import { get } from 'svelte/store';
import { api, type Game } from './api';
import { games } from './games';
import { notes, refreshNotes, type Notes } from './notes';
import { prefs, refreshPrefs, type PrefsMap } from './prefs';

/** Old number -> new id for emulator games whose old number still keys a note or a pref on the hub. */
export function remapPairs(list: Game[], n: Notes, p: PrefsMap): Record<string, string> {
  const out: Record<string, string> = {};
  for (const g of list)
    if (g.source === 'emu' && g.id && g.legacyId && g.legacyId !== g.id && (g.legacyId in n.games || g.legacyId in p))
      out[g.legacyId] = g.id;
  return out;
}

const tried = new Set<string>();

/** Send the pairs not tried yet. A failure (an old hub has no such route) is not retried: one attempt per pair per page load. */
export async function remapOnce(
  pairs: Record<string, string>,
  d: {
    post: (map: Record<string, string>) => Promise<{ notes: number; prefs: number }>;
    done: () => Promise<unknown>;
    seen?: Set<string>;
  } = {
    post: api.remapIds,
    done: () => Promise.all([refreshNotes(), refreshPrefs()]),
  },
): Promise<void> {
  const seen = d.seen ?? tried;
  const fresh = Object.fromEntries(Object.entries(pairs).filter(([k, v]) => !seen.has(`${k}>${v}`)));
  if (!Object.keys(fresh).length) return;
  for (const [k, v] of Object.entries(fresh)) seen.add(`${k}>${v}`);
  try {
    const r = await d.post(fresh);
    if (r.notes || r.prefs) await d.done();
  } catch {}
}

/** Look at the three stores whenever one changes. Needs games, notes and prefs to have loaded. */
export function watchRemap(): () => void {
  const check = () => void remapOnce(remapPairs(get(games)?.games ?? [], get(notes), get(prefs)));
  const offs = [games.subscribe(check), notes.subscribe(check), prefs.subscribe(check)];
  return () => offs.forEach((f) => f());
}
