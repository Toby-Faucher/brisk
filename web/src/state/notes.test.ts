import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { api, ApiError, type Game } from './api';
import {
  gameTags,
  newTagId,
  noteSaver,
  notes,
  notesLoaded,
  refreshNotes,
  TAG_COLORS,
  TAG_SYMBOLS,
  type GameNote,
  type Notes,
  withGame,
} from './notes';
import { filterByTag } from '../modules/library/library';

afterEach(() => {
  vi.useRealTimers();
  notes.set({ tags: {}, games: {} });
});

const tag = (name: string) => ({ name, color: 'teal' as const, symbol: 'users', note: '' });
const map: Notes = {
  tags: { coop: tag('Co-op night'), back: tag('Backlog') },
  games: { '620': { note: '', tags: ['gone', 'coop'] }, '400': { note: 'x', tags: ['back'] } },
};
const g = (appid: number): Game => ({
  appid,
  name: `G${appid}`,
  installed: true,
  sizeBytes: null,
  lastPlayed: null,
  playtimeMin: 0,
  playtime2wkMin: 0,
  cover: '',
  hero: '',
  logo: '',
  achUnlocked: null,
  achTotal: null,
  updated: false,
  update: null,
  controller: null,
  coop: false,
  running: false,
});

describe('notes helpers', () => {
  it('gameTags skips ids that are not tags', () => {
    expect(gameTags(map, '620').map((t) => t.name)).toEqual(['Co-op night']);
    expect(gameTags(map, '620')[0].id).toBe('coop');
    expect(gameTags(map, '1')).toEqual([]);
  });
  it('reads notes under either key form: id, number, or the old emulator number', () => {
    const m: Notes = {
      tags: map.tags,
      games: {
        'steam:620': { note: '', tags: ['coop'] },
        '3907440760': { note: '', tags: ['back'] },
        '400': { note: 'x', tags: ['back'] },
      },
    };
    expect(gameTags(m, '620').map((t) => t.id)).toEqual(['coop']); // an old machine's key under a new hub
    expect(gameTags(m, { appid: 620, id: 'steam:620' }).map((t) => t.id)).toEqual(['coop']);
    expect(gameTags(m, { appid: 3907440760, id: 'emu:dolphin:mario-kart-wii', legacyId: '3907440760' }).map((t) => t.id)).toEqual(['back']);
    expect(gameTags(m, { appid: 400, id: 'steam:400' }).map((t) => t.id)).toEqual(['back']); // a hub that is still old
    expect(filterByTag([g(620), g(400), g(1)], m, 'coop').map((x) => x.appid)).toEqual([620]);
  });
  it('withGame writes under the key and drops the other forms of it', () => {
    const m = withGame({ tags: {}, games: { '620': { note: 'old', tags: [] } } }, 'steam:620', { note: 'new', tags: [] });
    expect(Object.keys(m.games)).toEqual(['steam:620']);
  });
  it('filterByTag: null returns all, a tag keeps the games that carry it', () => {
    const games = [g(620), g(400), g(1)];
    expect(filterByTag(games, map, null)).toBe(games);
    expect(filterByTag(games, map, 'coop').map((x) => x.appid)).toEqual([620]);
    expect(filterByTag(games, map, 'gone')).toEqual([]);
  });
  it('newTagId is a valid hostd tag id', () => {
    for (let i = 0; i < 50; i++) expect(newTagId()).toMatch(/^[a-z0-9-]{1,16}$/);
    expect(newTagId()).not.toBe(newTagId());
  });
  it('every color has a CSS value and the symbol list is fixed', () => {
    expect(Object.keys(TAG_COLORS)).toEqual(['gray', 'red', 'orange', 'yellow', 'green', 'teal', 'blue', 'purple', 'pink']);
    expect(TAG_SYMBOLS).toContain('gamepad');
    for (const s of TAG_SYMBOLS) expect(s).toMatch(/^[a-z-]{1,16}$/);
  });
});

describe('noteSaver', () => {
  const echo = async (_id: string, b: GameNote) => ({ note: b.note, tags: b.tags });
  const rig = (fail = false) => {
    const put = vi.fn(async (id: string, b: GameNote) => {
      if (fail) throw new Error('offline');
      return echo(id, b);
    });
    return { put, s: noteSaver({ put, delay: 800 }) };
  };
  const st = (s: ReturnType<typeof noteSaver>, id: string) => get(s.status)[id]?.st;

  it('three edits inside 800 ms send one PUT, then show saved', async () => {
    vi.useFakeTimers();
    const put = vi.fn(async (_id: string, b: GameNote) => ({ note: b.note + '!', tags: b.tags }));
    const s = noteSaver({ put, delay: 800 });
    s.save('5', { note: 'a', tags: [] });
    await vi.advanceTimersByTimeAsync(300);
    s.save('5', { note: 'ab', tags: [] });
    await vi.advanceTimersByTimeAsync(300);
    s.save('5', { note: 'abc', tags: [] });
    expect(get(notes).games['5'].note).toBe('abc'); // optimistic
    await vi.advanceTimersByTimeAsync(799);
    expect(put).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(put).toHaveBeenCalledExactlyOnceWith('5', { note: 'abc', tags: [] });
    await vi.waitFor(() => expect(st(s, '5')).toBe('saved'));
    expect(get(notes).games['5'].note).toBe('abc!'); // reconciled with the answer
  });

  it('a failed PUT keeps the draft and retries on the next edit or on focus', async () => {
    vi.useFakeTimers();
    const { put, s } = rig(true);
    s.save('5', { note: 'draft', tags: [] });
    await vi.advanceTimersByTimeAsync(800);
    await vi.waitFor(() => expect(st(s, '5')).toBe('failed'));
    expect(get(notes).games['5'].note).toBe('draft');
    // focus: the same draft again
    await s.retry();
    expect(put).toHaveBeenCalledTimes(2);
    expect(put.mock.calls[1]).toEqual(['5', { note: 'draft', tags: [] }]);
    // next edit: the newer draft
    s.save('5', { note: 'draft 2', tags: [] });
    await vi.advanceTimersByTimeAsync(800);
    expect(put).toHaveBeenCalledTimes(3);
    expect(put.mock.calls[2]).toEqual(['5', { note: 'draft 2', tags: [] }]);
    // works again: saved, nothing left to retry
    put.mockImplementation(echo);
    await s.retry();
    expect(st(s, '5')).toBe('saved');
    await s.retry();
    expect(put).toHaveBeenCalledTimes(4);
  });

  it('a game that failed twice is not lost when you edit another game', async () => {
    const { put, s } = rig(true);
    s.save('1', { note: 'A', tags: [] });
    await s.flush(); // A fails
    s.save('2', { note: 'B', tags: [] });
    await s.flush(); // A and B both fail
    expect(st(s, '1')).toBe('failed');
    expect(st(s, '2')).toBe('failed');
    put.mockImplementation(echo);
    await s.retry();
    const sent = put.mock.calls.slice(-2);
    expect(sent).toEqual(
      expect.arrayContaining([
        ['1', { note: 'A', tags: [] }],
        ['2', { note: 'B', tags: [] }],
      ]),
    );
    expect(st(s, '1')).toBe('saved');
    expect(st(s, '2')).toBe('saved');
    expect(get(notes).games['1'].note).toBe('A');
  });

  it('an answer to an older PUT does not overwrite newer text or say saved', async () => {
    let first!: (v: GameNote) => void;
    const put = vi.fn((_id: string, b: GameNote) =>
      put.mock.calls.length === 1 ? new Promise<GameNote>((r) => (first = r)) : echo(_id, b),
    );
    const s = noteSaver({ put, delay: 800 });
    s.save('5', { note: 'one', tags: [] });
    void s.flush(); // request 1 in flight
    await vi.waitFor(() => expect(put).toHaveBeenCalledTimes(1));
    s.save('5', { note: 'one two', tags: [] });
    const second = s.flush(); // request 2 waits behind request 1
    first({ note: 'one', tags: [] });
    await Promise.resolve();
    await Promise.resolve();
    expect(get(notes).games['5'].note).toBe('one two');
    expect(st(s, '5')).not.toBe('saved');
    await second;
    expect(put).toHaveBeenCalledTimes(2);
    expect(get(notes).games['5'].note).toBe('one two');
    expect(st(s, '5')).toBe('saved');
  });

  it('a refused PUT (400/409) is not retried on focus and says why', async () => {
    const put = vi.fn(async () => {
      throw new ApiError('too many notes or tags', 409);
    });
    const s = noteSaver({ put, delay: 800 });
    s.save('9', { note: 'x', tags: [] });
    await s.flush();
    expect(get(s.status)[9]).toEqual({ st: 'refused', msg: 'too many notes or tags' });
    await s.retry();
    expect(put).toHaveBeenCalledTimes(1);
    expect(get(notes).games['9'].note).toBe('x'); // the text stays
  });

  it('a refresh after a refused save shows the server version, not the rejected text', async () => {
    const put = vi.fn(async () => {
      throw new ApiError('note is too long', 400);
    });
    const s = noteSaver({ put, delay: 800 });
    s.save('8', { note: 'rejected', tags: [] });
    await s.flush();
    const spy = vi.spyOn(api, 'notes').mockResolvedValue({ tags: {}, games: { '8': { note: 'server', tags: [] } } });
    await refreshNotes();
    spy.mockRestore();
    notesLoaded.set(false);
    expect(get(notes).games['8'].note).toBe('server');
    expect(get(s.status)[8]).toEqual({ st: 'refused', msg: 'note is too long' });
  });

  it('flush sends a pending change at once', async () => {
    const { put, s } = rig();
    s.save('7', { note: 'x', tags: [] });
    await s.flush();
    expect(put).toHaveBeenCalledExactlyOnceWith('7', { note: 'x', tags: [] });
  });

  it('a blank answer removes the game from the store', async () => {
    notes.set({ tags: {}, games: { '7': { note: 'old', tags: [] } } });
    const { s } = rig();
    s.save('7', { note: '  ', tags: [] });
    await s.flush();
    expect(get(notes).games['7']).toBeUndefined();
  });
});

describe('refreshNotes', () => {
  it('marks the notes loaded only after a GET works', async () => {
    const spy = vi.spyOn(api, 'notes').mockRejectedValueOnce(new Error('down'));
    await refreshNotes();
    expect(get(notesLoaded)).toBe('failed');
    spy.mockResolvedValueOnce({ tags: {}, games: { '3': { note: 'n', tags: [] } } });
    await refreshNotes();
    expect(get(notesLoaded)).toBe(true);
    expect(get(notes).games['3'].note).toBe('n');
    spy.mockRestore();
  });
});
