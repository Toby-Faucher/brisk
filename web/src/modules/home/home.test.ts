import { describe, expect, it } from 'vitest';
import type { Game } from '../../state/api';
import { fmtAgo, fmtDuration, fmtTemp, fmtUptime, moveItem, pickResume, reconcileOrder } from './home';

const g = (appid: number, installed: boolean, lastPlayed: number | null, running = false): Game => ({ appid, name: `g${appid}`, installed, sizeBytes: null, lastPlayed, playtimeMin: 0, playtime2wkMin: 0, cover: '', running });

describe('home', () => {
  it('resume_picks_most_recent_played_installed_game', () => {
    expect(pickResume([g(1, true, 100), g(2, false, 900), g(3, true, 500), g(4, true, null)])?.appid).toBe(3);
    expect(pickResume([g(1, true, null), g(2, false, 5)])).toBeNull();
    expect(pickResume([])).toBeNull();
  });

  it('resume_prefers_the_running_game', () => {
    expect(pickResume([g(1, true, 900), g(2, true, 100, true), g(3, true, 500)])?.appid).toBe(2);
  });

  it('dash_for_null_values', () => {
    expect(fmtTemp(null)).toBe('–');
    expect(fmtTemp(47.6)).toBe('48 °C');
    expect(fmtUptime(null)).toBe('–');
    expect(fmtUptime(75_600)).toBe('21 h');
    expect(fmtUptime(2 * 86_400 + 3 * 3600)).toBe('2 d 3 h');
    expect(fmtUptime(600)).toBe('10 min');
    expect(fmtAgo(null, 0)).toBe('–');
    expect(fmtAgo(0, 38 * 60_000)).toBe('38 min ago');
    expect(fmtDuration(0, null)).toBe('live');
    expect(fmtDuration(0, 64 * 60_000)).toBe('1 h 04');
    expect(fmtDuration(0, 12 * 60_000)).toBe('12 min');
  });

  it('reconcile_keeps_saved_order_appends_new_drops_removed', () => {
    const cur = ['host', 'resume', 'quick', 'recent', 'library'];
    expect(reconcileOrder([], cur)).toEqual(cur);
    expect(reconcileOrder(['recent', 'host'], cur)).toEqual(['recent', 'host', 'resume', 'quick', 'library']);
    // removed ids (gone, agents) are dropped; duplicates count once
    expect(reconcileOrder(['gone', 'library', 'library', 'agents', 'quick'], cur)).toEqual(['library', 'quick', 'host', 'resume', 'recent']);
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
});
