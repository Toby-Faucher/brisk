import { describe, expect, it, vi } from 'vitest';
import type { Game } from '../../state/api';
import { fmtPlaytime, gamesHeader, layoutFor, listGames, loadSort, saveSort, stepIndex } from './modal';

const g = (o: Partial<Game>): Game => ({ appid: 1, name: 'A', installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 0, playtime2wkMin: 0, cover: '', hero: '', logo: '', running: false, achUnlocked: null, achTotal: null, updated: false, update: null, controller: null, coop: false, ...o });
const set = [g({ appid: 1, name: 'Zed', sizeBytes: 5e9, lastPlayed: 10, playtimeMin: 30 }), g({ appid: 2, name: 'alpha', sizeBytes: 2e10, lastPlayed: 50, playtimeMin: 600 }), g({ appid: 3, name: 'Mid', installed: false, sizeBytes: 9e9 }), g({ appid: 4, name: 'Beta' })];
const names = (l: Game[]) => l.map((x) => x.name);

describe('modal helpers', () => {
  it('listGames_filters_installed_and_search', () => {
    expect(names(listGames(set, '', 'name'))).toEqual(['alpha', 'Beta', 'Zed']);
    expect(names(listGames(set, ' ET', 'name'))).toEqual(['Beta']);
  });
  it('listGames_sorts_desc_by_number_with_name_ties', () => {
    expect(names(listGames(set, '', 'size'))).toEqual(['alpha', 'Zed', 'Beta']);
    expect(names(listGames(set, '', 'played'))).toEqual(['alpha', 'Zed', 'Beta']);
    expect(names(listGames(set, '', 'playtime'))).toEqual(['alpha', 'Zed', 'Beta']);
  });
  it('listGames_does_not_mutate', () => {
    const copy = [...set];
    listGames(set, '', 'size');
    expect(set).toEqual(copy);
  });
  it('gamesHeader_counts_and_totals', () => {
    expect(gamesHeader(listGames(set, '', 'name'))).toBe('3 games · 25 GB');
    expect(gamesHeader([g({})])).toBe('1 game');
    expect(gamesHeader([])).toBe('0 games');
  });
  it('layoutFor_by_width', () => {
    expect([1400, 1100, 900, 700, 400].map(layoutFor)).toEqual(['wide', 'wide', 'medium', 'medium', 'narrow']);
  });
  it('fmtPlaytime', () => {
    expect([0, 45, 90, 1200].map(fmtPlaytime)).toEqual(['Not played yet', '45 min', '1.5 h', '20 h']);
  });
  it('stepIndex_clamps', () => {
    expect(stepIndex(0, 'ArrowUp', 3)).toBe(0);
    expect(stepIndex(2, 'ArrowDown', 3)).toBe(2);
    expect(stepIndex(0, 'End', 3)).toBe(2);
    expect(stepIndex(1, 'a', 3)).toBeNull();
    expect(stepIndex(0, 'ArrowDown', 0)).toBeNull();
  });
  it('sort_survives_blocked_storage', () => {
    const mem = new Map<string, string>();
    vi.stubGlobal('sessionStorage', { getItem: (k: string) => mem.get(k) ?? null, setItem: (k: string, v: string) => void mem.set(k, v) });
    saveSort('size');
    expect(loadSort()).toBe('size');
    mem.set('brisk.machineGamesSort', 'junk');
    expect(loadSort()).toBe('name');
    vi.stubGlobal('sessionStorage', { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } });
    expect(loadSort()).toBe('name');
    expect(() => saveSort('size')).not.toThrow();
    vi.unstubAllGlobals();
  });
});
