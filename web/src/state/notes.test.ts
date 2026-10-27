import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { Game } from './api';
import { gameTags, newTagId, noteSaver, notes, TAG_COLORS, TAG_SYMBOLS, type GameNote, type Notes } from './notes';
import { filterByTag } from '../modules/library/library';

afterEach(() => {
  vi.useRealTimers();
  notes.set({ tags: {}, games: {} });
});

const tag = (name: string) => ({ name, color: 'teal' as const, symbol: 'users', note: '' });
const map: Notes = { tags: { coop: tag('Co-op night'), back: tag('Backlog') }, games: { '620': { note: '', tags: ['gone', 'coop'] }, '400': { note: 'x', tags: ['back'] } } };
const g = (appid: number): Game => ({ appid, name: `G${appid}`, installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 0, playtime2wkMin: 0, cover: '', hero: '', logo: '', achUnlocked: null, achTotal: null, updated: false, running: false });

describe('notes helpers', () => {
  it('gameTags skips ids that are not tags', () => {
    expect(gameTags(map, 620).map((t) => t.name)).toEqual(['Co-op night']);
    expect(gameTags(map, 620)[0].id).toBe('coop');
    expect(gameTags(map, 1)).toEqual([]);
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
  const rig = (fail = false) => {
    const put = vi.fn(async (_id: number, b: GameNote) => {
      if (fail) throw new Error('offline');
      return { note: b.note.trimEnd(), tags: b.tags };
    });
    return { put, s: noteSaver({ put, delay: 800 }) };
  };

  it('three edits inside 800 ms send one PUT, then show saved', async () => {
    vi.useFakeTimers();
    const { put, s } = rig();
    s.save(5, { note: 'a', tags: [] });
    await vi.advanceTimersByTimeAsync(300);
    s.save(5, { note: 'ab', tags: [] });
    await vi.advanceTimersByTimeAsync(300);
    s.save(5, { note: 'abc ', tags: [] });
    expect(get(notes).games['5'].note).toBe('abc '); // optimistic
    await vi.advanceTimersByTimeAsync(799);
    expect(put).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(put).toHaveBeenCalledExactlyOnceWith(5, { note: 'abc ', tags: [] });
    await vi.waitFor(() => expect(get(s.status)).toBe('saved'));
    expect(get(notes).games['5'].note).toBe('abc'); // reconciled with the answer
  });

  it('a failed PUT keeps the draft and retries on the next edit or on focus', async () => {
    vi.useFakeTimers();
    const { put, s } = rig(true);
    s.save(5, { note: 'draft', tags: [] });
    await vi.advanceTimersByTimeAsync(800);
    await vi.waitFor(() => expect(get(s.status)).toBe('failed'));
    expect(get(notes).games['5'].note).toBe('draft');
    // focus: the same draft again
    await s.retry();
    expect(put).toHaveBeenCalledTimes(2);
    expect(put.mock.calls[1]).toEqual([5, { note: 'draft', tags: [] }]);
    // next edit: the newer draft
    s.save(5, { note: 'draft 2', tags: [] });
    await vi.advanceTimersByTimeAsync(800);
    expect(put).toHaveBeenCalledTimes(3);
    expect(put.mock.calls[2]).toEqual([5, { note: 'draft 2', tags: [] }]);
    // works again: saved, nothing left to retry
    put.mockImplementation(async (_id, b) => b);
    await s.retry();
    expect(get(s.status)).toBe('saved');
    await s.retry();
    expect(put).toHaveBeenCalledTimes(4);
  });

  it('flush sends a pending change at once', async () => {
    const { put, s } = rig();
    s.save(7, { note: 'x', tags: [] });
    await s.flush();
    expect(put).toHaveBeenCalledExactlyOnceWith(7, { note: 'x', tags: [] });
  });

  it('an empty answer removes the game from the store', async () => {
    notes.set({ tags: {}, games: { '7': { note: 'old', tags: [] } } });
    const { s } = rig();
    s.save(7, { note: '  ', tags: [] });
    await s.flush();
    expect(get(notes).games['7']).toBeUndefined();
  });
});
