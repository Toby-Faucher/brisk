import { describe, expect, it, vi } from 'vitest';
import { attachShortcuts, rank, sequenceMatcher, type KeyLike } from './keys';
import type { CommandItem } from './registry';

const item = (label: string, id = label): CommandItem => ({ id, label, run() {} });
const key = (k: string, target: KeyLike['target'] = null): KeyLike => ({ key: k, target, preventDefault() {} });

describe('rank', () => {
  it('rank_prefers_prefix_over_substring', () => {
    const items = [item('Open Settings'), item('Launch Portal'), item('Library'), item('Gallery')];
    expect(rank('lib', items).map((i) => i.label)).toEqual(['Library']);
    expect(rank('port', items).map((i) => i.label)).toEqual(['Launch Portal']); // word start
    expect(rank('al', [item('Gallery'), item('Alpha')]).map((i) => i.label)).toEqual(['Alpha', 'Gallery']);
  });
  it('rank_empty_query_returns_routes_first', () => {
    const items = [item('Launch X', 'game:1'), item('Home', 'route:/'), item('Toggle stats', 'cmd:stats'), item('Library', 'route:/library')];
    expect(rank('', items).map((i) => i.id)).toEqual(['route:/', 'route:/library', 'game:1', 'cmd:stats']);
  });
  it('rank_handles_139_games_under_5ms', () => {
    const items = Array.from({ length: 139 }, (_, i) => item(`Launch Game Number ${i}`));
    const t = performance.now();
    rank('gnum', items);
    rank('game 1', items);
    expect(performance.now() - t).toBeLessThan(5);
  });
});

describe('shortcuts', () => {
  it('shortcut_g_then_l_goes_to_library', () => {
    const go = vi.fn();
    const m = sequenceMatcher({ 'g h': () => go('/'), 'g l': () => go('/library'), 'g s': () => go('/stream') });
    expect(m(key('g'))).toBe(true);
    expect(m(key('l'))).toBe(true);
    expect(go).toHaveBeenCalledWith('/library');
    m(key('l'));
    expect(go).toHaveBeenCalledTimes(1); // sequence was consumed
  });
  it('shortcuts_ignored_while_typing_in_an_input', () => {
    const go = vi.fn();
    const m = sequenceMatcher({ 'g l': go });
    const input = { tagName: 'INPUT' };
    m(key('g', input));
    m(key('l', input));
    expect(go).not.toHaveBeenCalled();
  });
  it('wrong_second_key_resets', () => {
    const go = vi.fn();
    const m = sequenceMatcher({ 'g l': go });
    m(key('g'));
    m(key('x'));
    m(key('l'));
    expect(go).not.toHaveBeenCalled();
  });
  it('attachShortcuts_adds_one_listener_and_disposer_removes_it', () => {
    const t = new EventTarget();
    const add = vi.spyOn(t, 'addEventListener');
    const rm = vi.spyOn(t, 'removeEventListener');
    const open = vi.fn();
    const off = attachShortcuts(t, { open, go: () => {} });
    expect(add).toHaveBeenCalledTimes(1);
    const press = (k: string, extra = {}) => t.dispatchEvent(Object.assign(new Event('keydown'), { key: k, ...extra }));
    press('/');
    press('k', { ctrlKey: true });
    expect(open).toHaveBeenCalledTimes(2);
    off();
    expect(rm).toHaveBeenCalledTimes(1);
    press('/');
    expect(open).toHaveBeenCalledTimes(2);
  });
});
