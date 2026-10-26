import { describe, expect, it } from 'vitest';
import type { Game } from '../../state/api';
import { badgeLabel, cardClick, filterGames, installLabel, playLabel, searchGames, shelves, visibleRange, wheelToX } from './library';

const DAY = 86_400;
const g = (appid: number, name: string, o: Partial<Game> = {}): Game => ({ appid, name, installed: false, sizeBytes: null, lastPlayed: null, playtimeMin: 0, cover: '', running: false, ...o });

describe('library', () => {
  it('search_folds_diacritics', () => {
    const list = [g(1, 'Pokémon Legends'), g(2, 'Stardew Valley'), g(3, 'Ōkami HD')];
    expect(searchGames(list, 'pokemon').map((x) => x.appid)).toEqual([1]);
    expect(searchGames(list, 'POKÉ').map((x) => x.appid)).toEqual([1]);
    expect(searchGames(list, 'okami').map((x) => x.appid)).toEqual([3]);
    expect(searchGames(list, 'zelda')).toEqual([]);
  });

  it('search_prefers_word_start', () => {
    const list = [g(1, 'Hades II', { playtimeMin: 900 }), g(2, 'Shades of Doom', { playtimeMin: 10 }), g(3, 'The Hades Club', { playtimeMin: 50 }), g(4, 'Hadestown', { playtimeMin: 5 })];
    // word starts first (ties by playtime), then plain substrings
    expect(searchGames(list, 'hades').map((x) => x.appid)).toEqual([1, 3, 4, 2]);
    // empty query keeps everything, most played first
    expect(searchGames(list, '  ').map((x) => x.appid)).toEqual([1, 3, 2, 4]);
  });

  it('filter_recent_is_played_in_last_30_days', () => {
    const now = 1_800_000_000_000; // ms
    const s = now / 1000; // lastPlayed is unix seconds
    const list = [g(1, 'a', { lastPlayed: s - 2 * DAY, installed: true }), g(2, 'b', { lastPlayed: s - 31 * DAY }), g(3, 'c'), g(4, 'd', { lastPlayed: s - 29 * DAY })];
    expect(filterGames(list, 'recent', now).map((x) => x.appid)).toEqual([1, 4]);
    expect(filterGames(list, 'installed', now).map((x) => x.appid)).toEqual([1]);
    expect(filterGames(list, 'not-installed', now).map((x) => x.appid)).toEqual([2, 3, 4]);
    expect(filterGames(list, 'all', now)).toHaveLength(4);
  });

  it('shelves_hero_is_latest_played_installed', () => {
    const list = [g(1, 'a', { installed: true, lastPlayed: 100 }), g(2, 'b', { lastPlayed: 900 }), g(3, 'c', { installed: true, lastPlayed: 500 }), g(4, 'd', { installed: true })];
    const s = shelves(list);
    expect(s.hero?.appid).toBe(3);
    expect(s.installed.map((x) => x.appid)).toEqual([1, 3, 4]); // input order kept
    expect(s.notInstalled.map((x) => x.appid)).toEqual([2]);
  });

  it('shelves_handle_zero_games', () => {
    expect(shelves([])).toEqual({ installed: [], notInstalled: [], hero: null });
    expect(shelves([g(1, 'a', { installed: true })]).hero).toBeNull(); // never played: no hero
  });

  it('139_games_render_at_most_40_card_nodes', () => {
    const stride = 152; // 138 px card + 14 px gap
    for (const total of [0, 3, 136, 139]) {
      for (let left = 0; left <= total * stride; left += 97) {
        const r = visibleRange(left, 1000, stride, total);
        expect(r.start).toBeGreaterThanOrEqual(0);
        expect(r.end).toBeLessThanOrEqual(total);
        expect(r.end - r.start).toBeLessThanOrEqual(20);
        // the visible cards are always inside the window
        const first = Math.floor(left / stride);
        if (first < total) expect(r.start).toBeLessThanOrEqual(first);
        expect(r.end).toBeGreaterThanOrEqual(Math.min(total, Math.ceil((left + 1000) / stride)));
      }
    }
    expect(visibleRange(0, 1000, stride, 3)).toEqual({ start: 0, end: 3 });
    // a very wide window still caps the mounted cards
    expect(visibleRange(0, 10_000, stride, 139).end).toBe(20);
  });

  it('not_installed_click_selects_and_never_installs', () => {
    const off = g(1, 'Rust', { sizeBytes: 12.4e9 });
    const on = g(2, 'Hades II', { installed: true });
    expect(cardClick(off, null)).toEqual({ action: 'select', selected: 1 });
    expect(cardClick(off, 1)).toEqual({ action: 'select', selected: null }); // second click collapses
    expect(cardClick(g(3, 'x'), 1)).toEqual({ action: 'select', selected: 3 }); // moves the selection
    expect(cardClick(on, 1)).toEqual({ action: 'play', selected: null }); // installed stays one click
    expect(installLabel(off)).toBe('Install 12 GB');
    expect(installLabel(g(4, 'y', { sizeBytes: 3.21e9 }))).toBe('Install 3.2 GB');
    expect(installLabel(g(5, 'z'))).toBe('Install');
  });

  it('wheel_maps_to_shelf_only_when_it_can_scroll', () => {
    // vertical wheel over a shelf with room to move: scroll it sideways
    expect(wheelToX(0, 100, 0, 1000, 3000)).toBe(100);
    expect(wheelToX(0, -100, 500, 1000, 3000)).toBe(-100);
    // at an edge, or a shelf that fits, or a mostly horizontal gesture: leave it to the page/browser
    expect(wheelToX(0, -100, 0, 1000, 3000)).toBeNull();
    expect(wheelToX(0, 100, 2000, 1000, 3000)).toBeNull();
    expect(wheelToX(0, 100, 0, 1000, 1000)).toBeNull();
    expect(wheelToX(80, 10, 0, 1000, 3000)).toBeNull();
  });

  it('running_game_resumes_and_is_badged', () => {
    expect(playLabel(g(1, 'a', { installed: true, running: true }))).toBe('Resume');
    expect(playLabel(g(1, 'a', { installed: true }))).toBe('Play');
    expect(badgeLabel(g(1, 'a', { installed: true, running: true }))).toBe('Running');
    expect(badgeLabel(g(1, 'a', { installed: true }))).toBe('Installed');
    expect(badgeLabel(g(1, 'a', { sizeBytes: 2e9 }))).toBe('2.0 GB');
    expect(badgeLabel(g(1, 'a'))).toBe('Not installed');
  });
});
