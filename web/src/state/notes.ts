import { writable } from 'svelte/store';
import { api, ApiError } from './api';
import { pick, without, type GameRef } from './gameId';

/** The colors hostd accepts (notes.rs COLORS). */
export type TagColor = 'gray' | 'red' | 'orange' | 'yellow' | 'green' | 'teal' | 'blue' | 'purple' | 'pink';
/** A tag you made. `symbol` names a glyph in TAG_SYMBOLS; an unknown one draws as a dot. Text only: never HTML. */
export type Tag = { name: string; color: TagColor; symbol: string; note: string; pinned?: boolean };
/** One game's note and tag ids. Text only: never HTML. */
export type GameNote = { note: string; tags: string[] };
/** GET /api/notes: tags by id, games by id (`steam:620`); a hub that is still old keys them by number. Read with `pick`. */
export type Notes = { tags: Record<string, Tag>; games: Record<string, GameNote> };

/** Symbol and border colors, 6:1 or more on the dark panels (tokens.css; the app has no light theme). */
export const TAG_COLORS: Record<TagColor, string> = {
  gray: 'var(--c-set)',
  red: 'var(--bad)',
  orange: 'var(--c-hist)',
  yellow: 'var(--warn)',
  green: 'var(--ok)',
  teal: '#2dd4bf',
  blue: 'var(--c-stream)',
  purple: 'var(--c-agent)',
  pink: '#f472b6',
};

/** Glyph names, drawn by TagChip.svelte. */
export const TAG_SYMBOLS: string[] = [
  'star',
  'heart',
  'flag',
  'check',
  'clock',
  'fire',
  'bookmark',
  'trophy',
  'skull',
  'gamepad',
  'users',
  'bolt',
  'moon',
  'gift',
  'question',
];

/** Shared by Library, the detail panel and the cards; filled by watchNotes. */
export const notes = writable<Notes>({ tags: {}, games: {} });

/** 10 random chars of a-z 0-9: a valid hostd tag id. */
export function newTagId(): string {
  const b = crypto.getRandomValues(new Uint8Array(10));
  return Array.from(b, (x) => (x % 36).toString(36)).join('');
}

/** The game's tags in their order, skipping ids that are not (or no longer) tags. */
export function gameTags(map: Notes, game: GameRef | string): (Tag & { id: string })[] {
  return (pick(map.games, game)?.tags ?? []).flatMap((id) => (map.tags[id] ? [{ ...map.tags[id], id }] : []));
}

/** How many games carry tag `id`. */
export const tagUses = (map: Notes, id: string) => Object.values(map.games).filter((g) => g.tags.includes(id)).length;

/** `map` with the game at `key` set to `g`, or removed when `g` is empty (as hostd does). Other forms of its key go. */
export function withGame(map: Notes, key: string, g: GameNote): Notes {
  const games = without(map.games, key);
  return { ...map, games: g.note.trim() || g.tags.length ? { ...games, [key]: g } : games };
}

/** `map` without tag `id`, taken off every game too (as hostd does). */
export function withoutTag(map: Notes, id: string): Notes {
  const { [id]: _, ...tags } = map.tags;
  let n: Notes = { ...map, tags };
  for (const [key, g] of Object.entries(map.games))
    if (g.tags.includes(id)) n = withGame(n, key, { ...g, tags: g.tags.filter((t) => t !== id) });
  return n;
}

// Games with a change not yet confirmed by hostd: a refresh keeps the local version of those.
const unsaved = new Map<string, GameNote>();

let fetchedAt = -Infinity;
let inflight: Promise<void> | null = null;

/** true once a GET worked; 'failed' while the first one has not. Editing waits for it, so a save never wipes notes it never saw. */
export const notesLoaded = writable<boolean | 'failed'>(false);

export function refreshNotes(): Promise<void> {
  return (inflight ??= api
    .notes()
    .catch(() => null)
    .then((r) => {
      if (!r) return void notesLoaded.update((l) => l || 'failed'); // retried on the next focus
      fetchedAt = Date.now();
      let n: Notes = { tags: r.tags ?? {}, games: r.games ?? {} };
      for (const [id, g] of unsaved) n = withGame(n, id, g);
      notes.set(n);
      notesLoaded.set(true);
    })
    .finally(() => (inflight = null)));
}

/** Fetch if never fetched (or older than a minute), then again on window focus at most once a minute. */
export function watchNotes(win: EventTarget = window): () => void {
  const onFocus = () => {
    void gameNoteSaver.retry(); // a note that failed to save keeps trying after its panel closed
    if (Date.now() - fetchedAt >= 60_000) void refreshNotes();
  };
  onFocus();
  win.addEventListener('focus', onFocus);
  return () => win.removeEventListener('focus', onFocus);
}

export const NOTE_SAVE_MS = 800;
/** Per game; no entry means nothing to report. `refused`: hostd said no (400/409/413), `msg` says why; not retried. */
export type SaveState = { st: 'saving' | 'saved' | 'failed' | 'refused'; msg?: string };

/**
 * Debounced save of games' notes and tags. The store updates at once and takes hostd's answer when it is the answer to
 * the latest change. A failed PUT keeps its change for the next `save` or `retry` (on focus); a refused one is not
 * retried. Every game with a change is kept; PUTs go one at a time, in order.
 */
export function noteSaver(d: { put?: (key: string, b: GameNote) => Promise<GameNote>; delay?: number } = {}) {
  const put = d.put ?? ((a: string, b: GameNote) => api.putGameNote(a, b));
  const status = writable<Record<string, SaveState>>({});
  const set = (key: string, s: SaveState | null) =>
    status.update((m) => {
      const { [key]: _, ...rest } = m;
      return s ? { ...rest, [key]: s } : rest;
    });
  const pending = new Map<string, GameNote>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let chain: Promise<void> = Promise.resolve();

  const send = (key: string, body: GameNote) =>
    put(key, body).then(
      (r) => {
        if (unsaved.get(key) !== body) return; // a newer change exists; its answer reconciles
        unsaved.delete(key);
        notes.update((n) => withGame(n, key, r));
        set(key, { st: 'saved' });
      },
      (e) => {
        if (unsaved.get(key) !== body) return;
        if (e instanceof ApiError && [400, 409, 413].includes(e.status)) {
          unsaved.delete(key); // a refresh must not overlay text hostd rejected (the panel keeps its draft)
          return set(key, { st: 'refused', msg: e.message });
        }
        pending.set(key, body);
        set(key, { st: 'failed' });
      },
    );

  const flush = (): Promise<void> => {
    clearTimeout(timer);
    if (!pending.size) return chain;
    const batch = [...pending];
    pending.clear();
    for (const [key] of batch) set(key, { st: 'saving' });
    chain = chain.then(async () => {
      for (const [key, body] of batch) await send(key, body);
    });
    return chain;
  };

  return {
    status,
    save(key: string, body: GameNote) {
      pending.set(key, body);
      unsaved.set(key, body);
      notes.update((n) => withGame(n, key, body));
      set(key, null);
      clearTimeout(timer);
      timer = setTimeout(flush, d.delay ?? NOTE_SAVE_MS);
    },
    flush,
    /** Send the changes that failed (or are waiting) now; nothing to send is a no-op. */
    retry: () => (pending.size ? flush() : chain),
  };
}

/** The one saver the detail panel uses, so a failed save outlives the panel and is retried on focus (watchNotes). */
export const gameNoteSaver = noteSaver();
