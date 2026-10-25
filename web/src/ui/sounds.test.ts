import { afterEach, describe, expect, it, vi } from 'vitest';
import { settings } from '../state/settings';
import { play } from './sounds';

afterEach(() => vi.unstubAllGlobals());

describe('sounds', () => {
  it('no_op_when_muted', () => {
    const ctor = vi.fn();
    vi.stubGlobal('AudioContext', ctor);
    settings.update((v) => ({ ...v, sounds: false }));
    play('click');
    play('connect');
    expect(ctor).not.toHaveBeenCalled();
    // And without AudioContext at all it does nothing, even unmuted.
    vi.unstubAllGlobals();
    vi.stubGlobal('AudioContext', undefined);
    settings.update((v) => ({ ...v, sounds: true }));
    expect(() => play('error')).not.toThrow();
  });
});
