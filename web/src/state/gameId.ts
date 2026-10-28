// The one place that knows both game id shapes: an old hostd sends only a numeric `appid`, a new one also `id`
// (`steam:620`, `emu:dolphin:mario-kart-wii`), `caps` and `legacyId`. Everything else keys games by `gameKey`.
import type { Caps, Game } from './api';

/** Old hostds gave emulator games numbers from here up. Only the fallbacks below read it. */
const OLD_EMU_FROM = 0xe0000000;

export type GameRef = Pick<Game, 'appid'> & Partial<Pick<Game, 'id' | 'legacyId' | 'caps' | 'source' | 'emulator'>>;

/** The client's key for a game: its id, else its number as text. */
export const gameKey = (g: Pick<Game, 'appid'> & Partial<Pick<Game, 'id'>>): string => g.id ?? String(g.appid);

/** What the source can do with a game. An old hostd sends none: an `emulator` game (or one in the old id range) has
 *  Controls and no Install, any other game is a Steam game. This is the only emulator test in the app. */
export function gameCaps(g: GameRef): Caps {
  if (g.caps) return g.caps;
  const emu = g.source ? g.source === 'emu' : g.emulator !== undefined || g.appid >= OLD_EMU_FROM;
  return { install: !emu, controls: emu, achievements: !emu, launchOptions: false, saves: false, screenshots: !emu, art: false };
}

/** A key as a Steam number text, when it is one (`steam:620` or `620`). */
const steamNum = (k: string) => /^(?:steam:)?([1-9]\d*)$/.exec(k)?.[1];
const isSteamNum = (n: string | undefined): n is string => n !== undefined && Number(n) < OLD_EMU_FROM;

/** The id form of a key: `620` becomes `steam:620`. An emulator number or an id stays as it is. */
export function toIdForm(key: string): string {
  const n = /^[1-9]\d*$/.test(key) ? key : undefined;
  return isSteamNum(n) ? `steam:${n}` : key;
}
/** The number form of a key: `steam:620` becomes `620`. An emulator id stays (an old hostd has none). */
export const toNumberForm = (key: string): string => (isSteamNum(steamNum(key)) ? steamNum(key)! : key);

/** Same game whichever form each key has (`620` and `steam:620`). */
export const sameKey = (a: string, b: string) => a === b || toNumberForm(a) === toNumberForm(b);

/** Every key a store may hold this game under: its key, the other Steam form, its old number. */
export function keyForms(g: GameRef | string): string[] {
  const key = typeof g === 'string' ? g : gameKey(g);
  const out = [key, toIdForm(key), toNumberForm(key)];
  if (typeof g !== 'string') out.push(String(g.appid), ...(g.legacyId ? [g.legacyId] : []));
  return [...new Set(out)];
}

/** Read a note or pref map (keyed by whatever form the hub holds) for one game. */
export function pick<T>(map: Record<string, T>, g: GameRef | string): T | undefined {
  for (const k of keyForms(g)) if (map[k] !== undefined) return map[k];
  return undefined;
}

/** Find a game by a key in either form (a session row, a `?open=` link). */
export const findGame = <G extends GameRef>(games: G[], key: string | null | undefined): G | undefined =>
  key == null ? undefined : games.find((g) => sameKey(gameKey(g), key));

/** The key of a session row or stream answer: `gameId`, else the number. null for the Desktop. */
export const rowKey = (r: { gameId?: string | null; appid?: number | null }): string | null =>
  r.gameId ?? (r.appid == null ? null : String(r.appid));

/** One path segment for a game id: the colon is encoded once. */
export const gameSegment = (key: string) => encodeURIComponent(key);

/** A name safe for CSS idents and ids (view-transition-name): `steam:620` becomes `steam_620`. */
export const cssSafe = (key: string) => key.replace(/[^A-Za-z0-9_-]/g, '_');

/** `map` without this game's entries under any form of its key. */
export function without<T>(map: Record<string, T>, g: GameRef | string): Record<string, T> {
  const gone = new Set(keyForms(g));
  return Object.fromEntries(Object.entries(map).filter(([k]) => !gone.has(k)));
}
