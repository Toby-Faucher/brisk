import { describe, expect, it, vi } from 'vitest';
import { listener } from './listen';

describe('listener', () => {
  it('removes every listener when the signal aborts', () => {
    const ac = new AbortController();
    const on = listener(ac.signal);
    const t = new EventTarget();
    const a = vi.fn();
    const b = vi.fn();
    on(t, 'blur', a);
    on(t, 'focus', b);
    t.dispatchEvent(new Event('blur'));
    t.dispatchEvent(new Event('focus'));
    ac.abort();
    t.dispatchEvent(new Event('blur'));
    t.dispatchEvent(new Event('focus'));
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
  });

  it('passes the passive default and lets one call override it', () => {
    const add = vi.fn();
    const t = { addEventListener: add } as unknown as EventTarget;
    const { signal } = new AbortController();
    const on = listener(signal, true);
    on(t, 'wheel', () => {});
    on(t, 'wheel', () => {}, { passive: false });
    expect(add.mock.calls[0][2]).toEqual({ passive: true, signal });
    expect(add.mock.calls[1][2]).toEqual({ passive: false, signal });
  });
});
