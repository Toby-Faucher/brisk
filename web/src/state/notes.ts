import { writable } from 'svelte/store';
import { api } from './api';

/** The colors hostd accepts (notes.rs COLORS). */
export type TagColor = 'gray' | 'red' | 'orange' | 'yellow' | 'green' | 'teal' | 'blue' | 'purple' | 'pink';
/** A tag you made. `symbol` names a glyph in TAG_SYMBOLS; an unknown one draws as a dot. Text only: never HTML. */
export type Tag = { name: string; color: TagColor; symbol: string; note: string };
/** One game's note and tag ids. Text only: never HTML. */
export type GameNote = { note: string; tags: string[] };
/** GET /api/notes: tags by id, games by appid. */
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
export const TAG_SYMBOLS: string[] = ['star', 'heart', 'flag', 'check', 'clock', 'fire', 'bookmark', 'trophy', 'skull', 'gamepad', 'users', 'bolt', 'moon', 'gift', 'question'];

/** Shared by Library, the detail panel and the cards; filled by watchNotes. */
export const notes = writable<Notes>({ tags: {}, games: {} });

/** 10 random chars of a-z 0-9: a valid hostd tag id. */
export function newTagId(): string {
  const b = crypto.getRandomValues(new Uint8Array(10));
  return Array.from(b, (x) => (x % 36).toString(36)).join('');
}

/** The game's tags in their order, skipping ids that are not (or no longer) tags. */
export function gameTags(map: Notes, appid: number): (Tag & { id: string })[] {
  return (map.games[appid]?.tags ?? []).flatMap((id) => (map.tags[id] ? [{ ...map.tags[id], id }] : []));
}

/** How many games carry tag `id`. */
export const tagUses = (map: Notes, id: string) => Object.values(map.games).filter((g) => g.tags.includes(id)).length;

/** `map` with game `appid` set to `g`, or removed when `g` is empty (as hostd does). */
export function withGame(map: Notes, appid: number, g: GameNote): Notes {
  const { [appid]: _, ...games } = map.games;
  return { ...map, games: g.note.trim() || g.tags.length ? { ...games, [appid]: g } : games };
}

/** `map` without tag `id`, taken off every game too (as hostd does). */
export function withoutTag(map: Notes, id: string): Notes {
  const { [id]: _, ...tags } = map.tags;
  let n: Notes = { ...map, tags };
  for (const [appid, g] of Object.entries(map.games)) if (g.tags.includes(id)) n = withGame(n, Number(appid), { ...g, tags: g.tags.filter((t) => t !== id) });
  return n;
}

// Games with a change not yet confirmed by hostd: a refresh keeps the local version of those.
const unsaved = new Map<number, GameNote>();

let fetchedAt = -Infinity;
let inflight: Promise<void> | null = null;

export function refreshNotes(): Promise<void> {
  return (inflight ??= api
    .notes()
    .catch(() => null)
    .then((r) => {
      if (!r) return; // retried on the next focus
      fetchedAt = Date.now();
      let n = r;
      for (const [id, g] of unsaved) n = withGame(n, id, g);
      notes.set(n);
    })
    .finally(() => (inflight = null)));
}

/** Fetch if never fetched (or older than a minute), then again on window focus at most once a minute. */
export function watchNotes(win: EventTarget = window): () => void {
  const onFocus = () => {
    if (Date.now() - fetchedAt >= 60_000) void refreshNotes();
  };
  onFocus();
  win.addEventListener('focus', onFocus);
  return () => win.removeEventListener('focus', onFocus);
}

export const NOTE_SAVE_MS = 800;
export type SaveStatus = 'idle' | 'saving' | 'saved' | 'failed';

/**
 * Debounced save of one game's note and tags. The store updates at once and takes hostd's answer when it comes.
 * A failed PUT keeps the change: the next `save` or a `retry` (on focus) sends it again. PUTs go one at a time, in order.
 */
export function noteSaver(d: { put?: (appid: number, b: GameNote) => Promise<GameNote>; delay?: number } = {}) {
  const put = d.put ?? ((a: number, b: GameNote) => api.putGameNote(a, b));
  const status = writable<SaveStatus>('idle');
  let pending: { appid: number; body: GameNote } | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let chain: Promise<void> = Promise.resolve();

  const flush = (): Promise<void> => {
    clearTimeout(timer);
    const p = pending;
    if (!p) return chain;
    pending = null;
    status.set('saving');
    chain = chain.then(() =>
      put(p.appid, p.body).then(
        (r) => {
          if (pending?.appid === p.appid) return; // a newer change is waiting; its answer will reconcile
          if (unsaved.get(p.appid) === p.body) unsaved.delete(p.appid);
          notes.update((n) => withGame(n, p.appid, r));
          if (!pending) status.set('saved');
        },
        () => {
          pending ??= p;
          status.set('failed');
        },
      ),
    );
    return chain;
  };

  return {
    status,
    save(appid: number, body: GameNote) {
      if (pending && pending.appid !== appid) void flush();
      pending = { appid, body };
      unsaved.set(appid, body);
      notes.update((n) => withGame(n, appid, body));
      status.set('idle');
      clearTimeout(timer);
      timer = setTimeout(flush, d.delay ?? NOTE_SAVE_MS);
    },
    flush,
    /** Send a change that failed (or is waiting) now; nothing to send is a no-op. */
    retry: () => (pending ? flush() : chain),
  };
}
