import { describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { emitCommand, onCommand } from './commands';
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
