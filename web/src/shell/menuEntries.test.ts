import { describe, expect, it, vi } from 'vitest';
import type { Game } from '../state/api';
import type { Notes } from '../state/notes';
import { openEntries, surpriseEntry, tagEntries } from './menuEntries';

const g = (appid: number, name: string, o: Partial<Game> = {}): Game => ({
  appid,
  name,
  installed: true,
  sizeBytes: null,
  lastPlayed: null,
  playtimeMin: 100,
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
  ...o,
});
const none: Notes = { tags: {}, games: {} };

describe('tagEntries', () => {
  it('lists your tags and the smart tags that match, with counts', () => {
    const go = vi.fn();
    const notes: Notes = { tags: { abc: { name: 'Co-op night', color: 'red', symbol: 'star', note: '' } }, games: {} };
    const es = tagEntries(notes, [g(1, 'A', { controller: 'full' }), g(2, 'B', { controller: 'full' })], go);
    expect(es.map((e) => e.label)).toEqual(['Smart tag: Controller (2)', 'Tag: Co-op night']);
    expect(es[1].tag?.symbol).toBe('star');
    es[1].run();
    es[0].run();
    expect(go.mock.calls).toEqual([['/library?tag=abc'], ['/library?tag=auto-controller']]);
  });
  it('hides smart tags nothing matches', () => {
    expect(tagEntries(none, [g(1, 'A')], vi.fn())).toEqual([]);
  });
});

describe('surpriseEntry', () => {
  const games = [g(1, 'Played', { playtimeMin: 500 }), g(2, 'Fresh one', { playtimeMin: 0 }), g(3, 'Fresh two', { playtimeMin: 10 })];
  it('opens a Backlog game picked with the injected rng', () => {
    const go = vi.fn();
    surpriseEntry(games, go, () => 0.99)!.run();
    surpriseEntry(games, go, () => 0)!.run();
    expect(go.mock.calls).toEqual([['/library?open=3'], ['/library?open=2']]);
  });
  it('is absent when the Backlog is empty', () => {
    expect(surpriseEntry([games[0]], vi.fn())).toBeNull();
  });
});

describe('openEntries', () => {
  const games = [g(1, 'Pokémon Legends'), g(2, 'Stardew Valley'), ...Array.from({ length: 12 }, (_, i) => g(10 + i, `Stars ${i}`))];
  it('finds games by folded name and opens their panel', () => {
    const go = vi.fn();
    const es = openEntries(games, 'pokemon', go);
    expect(es.map((e) => e.label)).toEqual(['Open Pokémon Legends']);
    es[0].run();
    expect(go).toHaveBeenCalledWith('/library?open=1');
  });
  it('is empty without a query and capped otherwise', () => {
    expect(openEntries(games, ' ', vi.fn())).toEqual([]);
    expect(openEntries(games, ' - : ', vi.fn())).toEqual([]); // folds to nothing
    expect(openEntries(games, 'star', vi.fn())).toHaveLength(8);
  });
});
