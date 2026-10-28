import { describe, expect, it, vi } from 'vitest';
import newFx from '../../../crates/brisk-hostd/tests/fixtures/stage1/games-new.json';
import type { Game } from './api';
import { remapOnce, remapPairs } from './remap';

const games = newFx.games as unknown as Game[];
const note = { note: 'n', tags: [] as string[] };
const prefs = { preset: 'fast', codec: 'h264', updated: 1 } as const;

describe('remapPairs', () => {
  it('lists an emulator game whose old number still keys a note or a pref', () => {
    expect(remapPairs(games, { tags: {}, games: { '3907440760': note } }, {})).toEqual({ '3907440760': 'emu:dolphin:mario-kart-wii' });
    expect(remapPairs(games, { tags: {}, games: {} }, { '3907440760': prefs })).toEqual({ '3907440760': 'emu:dolphin:mario-kart-wii' });
  });
  it('lists nothing when the hub holds nothing under an old number, or for Steam games', () => {
    expect(remapPairs(games, { tags: {}, games: { 'emu:dolphin:mario-kart-wii': note, '620': note } }, { 'steam:620': prefs })).toEqual({});
    expect(remapPairs([{ ...games[0], legacyId: '620' }], { tags: {}, games: { '620': note } }, {})).toEqual({});
  });
  it('lists nothing for a game from an old hostd (no id)', () => {
    expect(remapPairs([{ appid: 3907440760, emulator: 'dolphin' } as Game], { tags: {}, games: { '3907440760': note } }, {})).toEqual({});
  });
});

describe('remapOnce', () => {
  const pairs = { '3907440760': 'emu:dolphin:mario-kart-wii' };
  it('posts once however often it is asked, then refreshes notes and prefs', async () => {
    const post = vi.fn(async () => ({ notes: 1, prefs: 0 }));
    const done = vi.fn(async () => {});
    const seen = new Set<string>();
    await remapOnce(pairs, { post, done, seen });
    await remapOnce(pairs, { post, done, seen });
    await Promise.all([remapOnce(pairs, { post, done, seen }), remapOnce(pairs, { post, done, seen })]);
    expect(post).toHaveBeenCalledExactlyOnceWith(pairs);
    expect(done).toHaveBeenCalledTimes(1);
  });
  it('does not refresh when nothing moved, and does not retry a failure (an old hub has no route)', async () => {
    const seen = new Set<string>();
    const none = vi.fn(async () => ({ notes: 0, prefs: 0 }));
    const done = vi.fn(async () => {});
    await remapOnce(pairs, { post: none, done, seen });
    expect(done).not.toHaveBeenCalled();
    const bad = vi.fn(async () => Promise.reject(new Error('404')));
    const seen2 = new Set<string>();
    await remapOnce(pairs, { post: bad, done, seen: seen2 });
    await remapOnce(pairs, { post: bad, done, seen: seen2 });
    expect(bad).toHaveBeenCalledTimes(1);
  });
  it('sends only the pairs not tried yet, and nothing for an empty list', async () => {
    const post = vi.fn(async () => ({ notes: 0, prefs: 0 }));
    const seen = new Set<string>();
    await remapOnce({}, { post, done: async () => {}, seen });
    expect(post).not.toHaveBeenCalled();
    await remapOnce(pairs, { post, done: async () => {}, seen });
    await remapOnce({ ...pairs, '3907440761': 'emu:dolphin:other' }, { post, done: async () => {}, seen });
    expect(post).toHaveBeenLastCalledWith({ '3907440761': 'emu:dolphin:other' });
  });
});
