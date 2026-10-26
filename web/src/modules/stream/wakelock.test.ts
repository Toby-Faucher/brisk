import { describe, expect, it, vi } from 'vitest';
import { keepAwake } from './wakelock';

function fakes({ fail }: { fail?: Error } = {}) {
  const locks: { released: boolean; release: () => Promise<void> }[] = [];
  const request = vi.fn(async () => {
    if (fail) throw fail;
    const l = { released: false, release: vi.fn(async () => void (l.released = true)) };
    locks.push(l);
    return l;
  });
  const doc = Object.assign(new EventTarget(), { visibilityState: 'visible' as DocumentVisibilityState });
  const show = (v: DocumentVisibilityState) => ((doc.visibilityState = v), doc.dispatchEvent(new Event('visibilitychange')));
  return { nav: { wakeLock: { request } }, doc, request, locks, show };
}
const flush = () => new Promise((r) => setTimeout(r));

describe('keepAwake', () => {
  it('acquires when live and releases on disconnect', async () => {
    const f = fakes();
    const a = keepAwake(f.nav, f.doc);
    await flush();
    expect(f.request).not.toHaveBeenCalled();
    a.set(true);
    await flush();
    expect(f.request).toHaveBeenCalledWith('screen');
    a.set(false);
    expect(f.locks[0].released).toBe(true);
  });

  it('asks again when the tab is visible after the browser dropped the lock', async () => {
    const f = fakes();
    const a = keepAwake(f.nav, f.doc);
    a.set(true);
    await flush();
    f.locks[0].released = true; // what Chrome does when the tab hides
    f.show('hidden');
    await flush();
    expect(f.request).toHaveBeenCalledTimes(1);
    f.show('visible');
    await flush();
    expect(f.request).toHaveBeenCalledTimes(2);
    a.stop();
    expect(f.locks[1].released).toBe(true);
    f.show('visible');
    await flush();
    expect(f.request).toHaveBeenCalledTimes(2); // stopped: the listener is gone
  });

  it('never asks with the setting off (set(false)) or while hidden', async () => {
    const f = fakes();
    const a = keepAwake(f.nav, f.doc);
    a.set(false);
    f.show('visible');
    await flush();
    f.doc.visibilityState = 'hidden';
    a.set(true);
    await flush();
    expect(f.request).not.toHaveBeenCalled();
  });

  it('does not ask twice for one lock', async () => {
    const f = fakes();
    const a = keepAwake(f.nav, f.doc);
    a.set(true);
    a.set(true); // still asking
    await flush();
    a.set(true); // holding
    f.show('visible');
    await flush();
    expect(f.request).toHaveBeenCalledTimes(1);
  });

  it('releases a lock that arrives after it was turned off', async () => {
    const f = fakes();
    const a = keepAwake(f.nav, f.doc);
    a.set(true);
    a.set(false);
    await flush();
    expect(f.locks[0].released).toBe(true);
  });

  it('swallows refusals with one debug line and works without the API', async () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => {});
    const f = fakes({ fail: Object.assign(new Error('denied'), { name: 'NotAllowedError' }) });
    const a = keepAwake(f.nav, f.doc);
    a.set(true);
    await flush();
    f.show('visible');
    await flush();
    expect(f.request).toHaveBeenCalledTimes(2);
    expect(debug).toHaveBeenCalledTimes(1);
    const none = keepAwake({}, f.doc);
    expect(() => (none.set(true), none.stop())).not.toThrow();
    debug.mockRestore();
  });
});
