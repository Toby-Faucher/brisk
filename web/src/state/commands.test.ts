import { describe, expect, it, vi } from 'vitest';
import { emitCommand, onCommand } from './commands';

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
});
