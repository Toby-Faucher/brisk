import { describe, expect, it } from 'vitest';
import type { Game } from '../../state/api';
import {
  badgeLabel,
  filterByTag,
  almostDone,
  backlog,
  filterGames,
  pickRandom,
  smartTags,
  fmtFortnight,
  installLabel,
  playLabel,
  saveRow,
  savesLine,
  searchGames,
  shelves,
  sortGames,
  updateLabel,
  visibleRange,
  wheelToX,
} from './library';

const DAY = 86_400;
const g = (appid: number, name: string, o: Partial<Game> = {}): Game => ({
  appid,
  name,
  installed: false,
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
  ...o,
});

describe('library', () => {
  it('updateLabel_names_the_state', () => {
    expect(updateLabel(g(1, 'a', { update: 'needed' }))).toBe('Update needed');
    expect(updateLabel(g(1, 'a', { update: 'running' }))).toBe('Updating…');
    expect(updateLabel(g(1, 'a'))).toBeNull();
  });

  it('sortGames_orders_and_default_keeps_input', () => {
    const list = [
      g(1, 'banjo', { playtimeMin: 5, lastPlayed: 10 }),
      g(2, 'Apex', { playtimeMin: 50 }),
      g(3, 'cave', { playtimeMin: 9, lastPlayed: 99 }),
    ];
    const ids = (s: Parameters<typeof sortGames>[1]) => sortGames(list, s).map((x) => x.appid);
    expect(ids('default')).toEqual([1, 2, 3]);
    expect(ids('az')).toEqual([2, 1, 3]);
    expect(ids('most-played')).toEqual([2, 3, 1]);
    expect(ids('last-played')).toEqual([3, 1, 2]);
    expect(list.map((x) => x.appid)).toEqual([1, 2, 3]);
  });

  it('search_folds_diacritics', () => {
    const list = [g(1, 'Pokémon Legends'), g(2, 'Stardew Valley'), g(3, 'Ōkami HD')];
    expect(searchGames(list, 'pokemon').map((x) => x.appid)).toEqual([1]);
    expect(searchGames(list, 'POKÉ').map((x) => x.appid)).toEqual([1]);
    expect(searchGames(list, 'okami').map((x) => x.appid)).toEqual([3]);
    expect(searchGames(list, 'zelda')).toEqual([]);
  });

  it('search_prefers_word_start', () => {
    const list = [
      g(1, 'Hades II', { playtimeMin: 900 }),
      g(2, 'Shades of Doom', { playtimeMin: 10 }),
      g(3, 'The Hades Club', { playtimeMin: 50 }),
      g(4, 'Hadestown', { playtimeMin: 5 }),
    ];
    // word starts first (ties by playtime), then plain substrings
    expect(searchGames(list, 'hades').map((x) => x.appid)).toEqual([1, 3, 4, 2]);
    // empty query keeps everything, most played first
    expect(searchGames(list, '  ').map((x) => x.appid)).toEqual([1, 3, 2, 4]);
  });

  it('filter_recent_is_played_in_last_30_days', () => {
    const now = 1_800_000_000_000; // ms
    const s = now / 1000; // lastPlayed is unix seconds
    const list = [
      g(1, 'a', { lastPlayed: s - 2 * DAY, installed: true }),
      g(2, 'b', { lastPlayed: s - 31 * DAY }),
      g(3, 'c'),
      g(4, 'd', { lastPlayed: s - 29 * DAY }),
    ];
    expect(filterGames(list, 'recent', now).map((x) => x.appid)).toEqual([1, 4]);
    expect(filterGames(list, 'installed', now).map((x) => x.appid)).toEqual([1]);
    expect(filterGames(list, 'not-installed', now).map((x) => x.appid)).toEqual([2, 3, 4]);
    expect(filterGames(list, 'all', now)).toHaveLength(4);
  });

  it('shelves_hero_is_latest_played_installed', () => {
    const list = [
      g(1, 'a', { installed: true, lastPlayed: 100 }),
      g(2, 'b', { lastPlayed: 900 }),
      g(3, 'c', { installed: true, lastPlayed: 500 }),
      g(4, 'd', { installed: true }),
    ];
    const s = shelves(list);
    expect(s.hero?.appid).toBe(3);
    expect(s.installed.map((x) => x.appid)).toEqual([1, 3, 4]); // input order kept
    expect(s.notInstalled.map((x) => x.appid)).toEqual([2]);
  });

  it('shelves_fortnight_orders_most_first', () => {
    const list = [g(730, 'cs', { playtime2wkMin: 30 }), g(220, 'hl'), g(620, 'p2', { playtime2wkMin: 192 })];
    expect(shelves(list).fortnight.map((x) => x.appid)).toEqual([620, 730]);
  });

  it('fmtFortnight_hours_or_minutes', () => {
    expect(fmtFortnight(192)).toBe('3.2 h this fortnight');
    expect(fmtFortnight(25)).toBe('25 min this fortnight');
  });

  it('shelves_handle_zero_games', () => {
    expect(shelves([])).toEqual({ installed: [], notInstalled: [], fortnight: [], hero: null });
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

  it('install_label_carries_the_size', () => {
    const off = g(1, 'Rust', { sizeBytes: 12.4e9 });
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

  it('savesLine_counts_size_and_age', () => {
    const now = 1_000_000_000_000;
    expect(savesLine({ files: 14, bytes: 2_100_000, modified: now / 1000 - 3 * DAY }, now)).toBe(
      'Cloud saves: 14 files, 2.1 MB, synced 3 d ago',
    );
    expect(savesLine({ files: 1, bytes: 400, modified: 0 }, now)).toBe('Cloud saves: 1 file, 1 KB');
    expect(savesLine({ files: 3, bytes: 5_200_000_000, modified: now / 1000 - 120 }, now)).toBe(
      'Cloud saves: 3 files, 5.2 GB, synced 2 min ago',
    );
    expect(savesLine({ files: 0, bytes: 0, modified: 0 }, now)).toBeNull();
    expect(savesLine(null, now)).toBeNull();
  });

  it('filterByTag_composes_with_search', () => {
    const list = [g(1, 'Hades II'), g(2, 'Hadestown'), g(3, 'Portal 2')];
    const n = {
      tags: { coop: { name: 'Co-op', color: 'teal' as const, symbol: 'users', note: '' } },
      games: { '2': { note: '', tags: ['coop'] }, '3': { note: '', tags: ['coop'] } },
    };
    expect(searchGames(filterByTag(list, n, 'coop'), 'hades').map((x) => x.appid)).toEqual([2]);
    expect(filterByTag(searchGames(list, 'hades'), n, 'coop').map((x) => x.appid)).toEqual([2]);
    expect(filterByTag(searchGames(list, 'hades'), n, null).map((x) => x.appid)).toEqual([1, 2]);
  });

  const tagIds = (o: Partial<Game>) => smartTags(g(1, 'x', o)).map((t) => t.id);

  it('backlog_is_under_30_minutes', () => {
    expect(tagIds({ playtimeMin: 29 })).toContain('auto-backlog');
    expect(tagIds({ playtimeMin: 30 })).not.toContain('auto-backlog');
    expect(tagIds({ playtimeMin: 0 })).toContain('auto-backlog');
  });

  it('controller_needs_full_support', () => {
    expect(tagIds({ playtimeMin: 99, controller: 'full' })).toEqual(['auto-controller']);
    expect(tagIds({ playtimeMin: 99, controller: 'partial' })).toEqual([]);
  });

  it('almost_done_is_80_to_99_percent', () => {
    const at = (u: number, t: number) => tagIds({ playtimeMin: 99, achUnlocked: u, achTotal: t }).includes('auto-almost');
    expect(at(79, 100)).toBe(false);
    expect(at(80, 100)).toBe(true);
    expect(at(99, 100)).toBe(true);
    expect(at(100, 100)).toBe(false);
    expect(at(0, 0)).toBe(false);
  });

  it('smartTags_orders_backlog_first', () => {
    expect(tagIds({ coop: true, updated: true, controller: 'full' })).toEqual([
      'auto-backlog',
      'auto-controller',
      'auto-coop',
      'auto-updated',
    ]);
  });

  it('filterByTag_applies_auto_rule', () => {
    const list = [g(1, 'a', { playtimeMin: 5 }), g(2, 'b', { playtimeMin: 500 }), g(3, 'c', { playtimeMin: 0 })];
    const none = { tags: {}, games: {} };
    expect(filterByTag(list, none, 'auto-backlog').map((x) => x.appid)).toEqual([1, 3]);
    expect(filterByTag(list, none, 'auto-nope')).toEqual([]);
  });

  it('pickRandom_empty_is_null', () => {
    expect(pickRandom([])).toBeNull();
  });

  it('pickRandom_uses_rng', () => {
    expect(pickRandom([1, 2, 3], () => 0.99)).toBe(3);
    expect(pickRandom([1, 2, 3], () => 0)).toBe(1);
  });

  it('shelves_backlog_and_almost_done_order', () => {
    const list = [
      g(1, 'b', { playtimeMin: 10 }),
      g(2, 'a', { playtimeMin: 10 }),
      g(3, 'c', { playtimeMin: 0 }),
      g(4, 'd', { playtimeMin: 99 }),
    ];
    expect(backlog(list).map((x) => x.appid)).toEqual([3, 2, 1]);
    const ach = [
      g(1, 'x', { achUnlocked: 85, achTotal: 100 }),
      g(2, 'y', { achUnlocked: 99, achTotal: 100 }),
      g(3, 'z', { achUnlocked: 50, achTotal: 100 }),
    ];
    expect(almostDone(ach).map((x) => x.appid)).toEqual([2, 1]);
  });

  it('backlog_shelf_is_capped_at_40', () => {
    const many = Array.from({ length: 50 }, (_, i) => g(i + 1, `g${i}`, { playtimeMin: i % 10 }));
    expect(backlog(many)).toHaveLength(40);
    expect(backlog(many)[0].playtimeMin).toBe(0);
  });
});

describe('saveRow', () => {
  const now = 1_000_000_000_000;
  it('maps each kind to an icon and a plain detail line', () => {
    expect(
      saveRow({ kind: 'state', name: 'RMCE01.s01', path: 'StateSaves/RMCE01.s01', size: 4_100_000, modified: now / 1000 - 3 * DAY }, now),
    ).toEqual({
      icon: 'replay',
      title: 'RMCE01.s01',
      detail: 'Save state, 4.1 MB, 3 d ago',
      note: null,
    });
    expect(saveRow({ kind: 'nand-save', name: 'Wii save data', path: 'x', size: 150, modified: null, files: 1 }, now).detail).toBe(
      'Wii save, 1 file, 1 KB',
    );
    const c = saveRow(
      { kind: 'memory-card', name: 'MemoryCardA.USA.raw', path: 'x', size: 524_288, modified: now / 1000, note: 'Shared' },
      now,
    );
    expect([c.icon, c.note]).toEqual(['clip', 'Shared']);
  });
});
