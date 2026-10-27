import type { GameNote, Notes, Tag, TagColor } from './notes';

/** Biggest file Restore reads. */
export const MAX_BACKUP_BYTES = 2 * 1024 * 1024;

const COLORS: TagColor[] = ['gray', 'red', 'orange', 'yellow', 'green', 'teal', 'blue', 'purple', 'pink'];

export type Backup = { format: 'brisk-notes'; version: 1; exported: string } & Notes;

/** The download: what GET /api/notes returned, with a header. */
export const buildBackup = (n: Notes, now: Date): Backup => ({ format: 'brisk-notes', version: 1, exported: now.toISOString(), tags: n.tags, games: n.games });

// hostd's limits (notes.rs): text lengths count Unicode characters, trailing whitespace not counted
const len = (s: string) => [...s.trimEnd()].length;
const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

class Bad extends Error {}
const bad = (m: string): never => {
  throw new Bad(m);
};

function parseTag(id: string, v: unknown): Tag {
  if (!/^[a-z0-9-]{1,16}$/.test(id)) bad(`"${id}" is not a valid tag id (1 to 16 of a-z, 0-9 and -).`);
  if (id.startsWith('auto-')) bad(`Tag id "${id}" starts with "auto-", which is reserved for smart tags.`);
  if (!isObj(v)) return bad(`Tag ${id} is not an object.`);
  const name = typeof v.name === 'string' ? v.name.trim() : '';
  if (!name || [...name].length > 24) bad(`Tag ${id} needs a name of 1 to 24 characters.`);
  if (!COLORS.includes(v.color as TagColor)) bad(`Tag ${id} has an unknown color.`);
  if (typeof v.symbol !== 'string' || !/^[a-z-]{1,16}$/.test(v.symbol)) bad(`Tag ${id} has an invalid symbol (1 to 16 of a-z and -).`);
  if (typeof v.note !== 'string' || len(v.note) > 500) bad(`The note of tag ${id} must be text of at most 500 characters.`);
  if (v.pinned !== undefined && typeof v.pinned !== 'boolean') bad(`Tag ${id} has an invalid pinned value.`);
  const t: Tag = { name, color: v.color as TagColor, symbol: v.symbol as string, note: v.note as string };
  if (v.pinned !== undefined) t.pinned = v.pinned as boolean;
  return t;
}

function parseGame(appid: string, v: unknown, tags: Record<string, Tag>): GameNote | null {
  if (!/^[1-9]\d*$/.test(appid) || !Number.isSafeInteger(Number(appid))) bad(`"${appid}" is not a valid appid (a positive whole number).`);
  if (!isObj(v)) return bad(`Game ${appid} is not an object.`);
  if (typeof v.note !== 'string' || len(v.note) > 4000) bad(`The note of game ${appid} must be text of at most 4000 characters.`);
  if (!Array.isArray(v.tags) || v.tags.some((t) => typeof t !== 'string')) bad(`The tags of game ${appid} must be a list of tag ids.`);
  const ids = [...new Set(v.tags as string[])].filter((t) => Object.hasOwn(tags, t)); // unknown ids are dropped, as hostd does
  if (ids.length > 16) bad(`Game ${appid} has more than 16 tags.`);
  return !(v.note as string).trim() && !ids.length ? null : { note: v.note as string, tags: ids };
}

/** Checks a backup file strictly; the first problem found is the error. Unknown fields are ignored. */
export function parseBackup(text: string): { ok: true; tags: Record<string, Tag>; games: Record<string, GameNote> } | { ok: false; error: string } {
  try {
    if (text.length > MAX_BACKUP_BYTES) bad('That file is bigger than 2 MiB.');
    let j: unknown;
    try {
      j = JSON.parse(text);
    } catch {
      return bad('That file is not valid JSON.');
    }
    if (!isObj(j) || j.format !== 'brisk-notes') bad('That is not a brisk notes backup.');
    const o = j as Record<string, unknown>;
    if (o.version !== 1) bad('That backup has an unsupported version.');
    const rawTags = o.tags ?? {};
    const rawGames = o.games ?? {};
    if (!isObj(rawTags)) bad('The tags in that file are not an object.');
    if (!isObj(rawGames)) bad('The games in that file are not an object.');
    const tagEntries = Object.entries(rawTags as object);
    const gameEntries = Object.entries(rawGames as object);
    if (tagEntries.length > 64) bad('That file has more than 64 tags.');
    if (gameEntries.length > 2000) bad('That file has more than 2000 games.');
    const tags: Record<string, Tag> = {};
    for (const [id, v] of tagEntries) tags[id] = parseTag(id, v);
    const games: Record<string, GameNote> = {};
    for (const [appid, v] of gameEntries) {
      const g = parseGame(appid, v, tags);
      if (g) games[appid] = g;
    }
    return { ok: true, tags, games };
  } catch (e) {
    if (e instanceof Bad) return { ok: false, error: e.message };
    throw e;
  }
}
