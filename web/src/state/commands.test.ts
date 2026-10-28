import { describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { emitCommand, machineEntries, onCommand } from './commands';
import type { Machine } from './machines';
import { settings } from './settings';

describe('command bus', () => {
  it('delivers_to_subscribers_until_unsubscribed', () => {
    const fn = vi.fn();
    const off = onCommand(fn);
    emitCommand({ type: 'stats' });
    emitCommand({ type: 'preset', preset: 'crisp' });
    expect(fn.mock.calls.map((c) => c[0])).toEqual([{ type: 'stats' }, { type: 'preset', preset: 'crisp' }]);
    off();
    emitCommand({ type: 'stats' });
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it('sets_the_default_preset_when_nobody_listens', () => {
    expect(emitCommand({ type: 'preset', preset: 'fast' })).toBe(false);
    expect(get(settings).defaultPreset).toBe('fast');
    const off = onCommand(() => {});
    expect(emitCommand({ type: 'preset', preset: 'crisp' })).toBe(true);
    expect(get(settings).defaultPreset).toBe('fast');
    off();
  });
});

describe('machineEntries', () => {
  const m = (id: string, state: Machine['state'], canPower = true): Machine => ({ id, name: id.toUpperCase(), state, canPower, sunshine: null, host: null, url: '', rttMs: null, vm: null });
  it('offers_switch_for_others_and_start_for_off_power_machines', () => {
    const act = { select: vi.fn(), start: vi.fn() };
    const es = machineEntries([m('a', 'ready'), m('b', 'off'), m('c', 'off', false)], 'a', act);
    expect(es.map((e) => e.label)).toEqual(['Switch to B', 'Start B', 'Switch to C']);
    es[0].run();
    es[1].run();
    expect(act.select).toHaveBeenCalledWith('b');
    expect(act.start).toHaveBeenCalledWith('b');
  });
});
