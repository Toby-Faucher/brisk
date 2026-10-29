import { describe, expect, it } from 'vitest';
import type { Game } from '../../state/api';
import { moveItem, orderToStore, pickResume, reconcileOrder, tagCardIds } from './home';

const g = (appid: number, installed: boolean, lastPlayed: number | null, running = false): Game => ({
  appid,
  name: `g${appid}`,
  installed,
  sizeBytes: null,
  lastPlayed,
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
  running,
});

describe('home', () => {
  it('resume_picks_most_recent_played_installed_game', () => {
    expect(pickResume([g(1, true, 100), g(2, false, 900), g(3, true, 500), g(4, true, null)])?.appid).toBe(3);
    expect(pickResume([g(1, true, null), g(2, false, 5)])).toBeNull();
    expect(pickResume([])).toBeNull();
  });

  it('resume_can_pick_a_recently_launched_emulator_game_over_an_older_steam_one', () => {
    const emu = { ...g(0xe0000001, true, 900), emulator: 'dolphin' };
    expect(pickResume([g(1, true, 100), emu])?.appid).toBe(0xe0000001);
  });

  it('resume_prefers_the_running_game', () => {
    expect(pickResume([g(1, true, 900), g(2, true, 100, true), g(3, true, 500)])?.appid).toBe(2);
  });

  it('reconcile_keeps_saved_order_appends_new_drops_removed', () => {
    const cur = ['host', 'resume', 'quick', 'recent', 'library'];
    expect(reconcileOrder([], cur)).toEqual(cur);
    expect(reconcileOrder(['recent', 'host'], cur)).toEqual(['recent', 'host', 'resume', 'quick', 'library']);
    // removed ids (gone, agents) are dropped; duplicates count once
    expect(reconcileOrder(['gone', 'library', 'library', 'agents', 'quick'], cur)).toEqual([
      'library',
      'quick',
      'host',
      'resume',
      'recent',
    ]);
    expect(reconcileOrder(['a', 'b'], [])).toEqual([]);
    expect(reconcileOrder(cur.slice().reverse(), cur)).toEqual(cur.slice().reverse());
  });

  it('move_item_moves_and_clamps_without_mutating', () => {
    const a = ['a', 'b', 'c', 'd'];
    expect(moveItem(a, 0, 2)).toEqual(['b', 'c', 'a', 'd']);
    expect(moveItem(a, 3, 0)).toEqual(['d', 'a', 'b', 'c']);
    expect(moveItem(a, 1, 1)).toEqual(a);
    expect(moveItem(a, 1, -5)).toEqual(['b', 'a', 'c', 'd']);
    expect(moveItem(a, 1, 99)).toEqual(['a', 'c', 'd', 'b']);
    expect(moveItem(a, 7, 0)).toEqual(a);
    expect(a).toEqual(['a', 'b', 'c', 'd']);
  });

  const tg = (name: string, pinned?: boolean) => ({ name, color: 'red' as const, symbol: 'star', note: '', pinned });
  it('tagCardIds only pinned and sorted', () => {
    const tags = { zz: tg('alpha', true), b1: tg('Beta', true), a1: tg('beta', true), off: tg('Gamma', false), none: tg('Delta') };
    expect(tagCardIds(tags)).toEqual(['tag-zz', 'tag-a1', 'tag-b1']);
  });

  it('tagCardIds ids pass clean_home_order rules (a-z0-9- , max 32)', () => {
    const id = 'a-b-c-0123456789'.slice(0, 16);
    const [card] = tagCardIds({ [id]: tg('x', true) });
    expect(card).toMatch(/^[a-z0-9-]{1,32}$/);
    expect(card.length).toBeLessThanOrEqual(20);
    for (const fixed of ['host', 'resume', 'quick', 'recent', 'friends', 'add']) expect(card).not.toBe(fixed);
  });

  it('a saved order keeps the place of tag cards that exist and drops gone ones', () => {
    const cur = ['host', 'tag-a', 'resume'];
    expect(reconcileOrder(['tag-a', 'tag-gone', 'host'], cur)).toEqual(['tag-a', 'host', 'resume']);
  });
});

describe('orderToStore', () => {
  const next = ['host', 'quick'];
  const saved = ['tag-a', 'host', 'tag-b', 'quick'];
  it('keeps saved tag-* ids while notes are not loaded', () => {
    expect(orderToStore(next, saved, false)).toEqual(['host', 'quick', 'tag-a', 'tag-b']);
    expect(orderToStore(['tag-a', 'host'], saved, false)).toEqual(['tag-a', 'host', 'tag-b']);
  });
  it('drops a vanished tag once notes are loaded', () => {
    expect(orderToStore(next, saved, true)).toEqual(next);
  });
});
