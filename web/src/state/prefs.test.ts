import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { StreamReq } from './api';
import { forgetPrefs, prefs, prefsChip, prefsFor, prefSaver, savedBody, startGame, type GamePrefs, type PrefsMap } from './prefs';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  prefs.set({});
});

const P = (o: Partial<GamePrefs> = {}): GamePrefs => ({ preset: 'crisp', codec: 'hevc', updated: 1, ...o });

describe('prefsFor', () => {
  it('uses the saved prefs of that game, else the defaults (balanced, h264)', () => {
    const map: PrefsMap = { '10': P(), '20': P({ preset: 'fast', codec: 'h264' }) };
    expect(prefsFor(10, map, 'balanced')).toEqual({ preset: 'crisp', codec: 'hevc', appid: 10 });
    expect(prefsFor(20, map, 'crisp')).toEqual({ preset: 'fast', codec: 'h264', appid: 20 });
    expect(prefsFor(30, map, 'crisp')).toEqual({ preset: 'crisp', codec: 'h264', appid: 30 });
    expect(prefsFor(30, {}, 'balanced')).toEqual({ preset: 'balanced', codec: 'h264', appid: 30 });
  });
  it('carries the custom fields only for custom', () => {
    const c = P({ preset: 'custom', codec: 'h264', width: 2560, height: 1440, fps: 120, bitrateKbps: 50000 });
    expect(prefsFor(1, { '1': c }, 'fast')).toEqual({
      preset: 'custom',
      codec: 'h264',
      width: 2560,
      height: 1440,
      fps: 120,
      bitrateKbps: 50000,
      appid: 1,
    });
    expect(prefsFor(1, { '1': P({ width: 5 }) }, 'fast')).not.toHaveProperty('width');
  });
  it('without an appid (Desktop) never looks anything up', () => {
    expect(prefsFor(undefined, { '0': P(), undefined: P() } as PrefsMap, 'fast')).toEqual({ preset: 'fast', codec: 'h264' });
  });
});

describe('savedBody', () => {
  it('a preset chip keeps the live codec; Advanced names its own and its numbers', () => {
    expect(savedBody({ preset: 'crisp' }, 'hevc')).toEqual({ preset: 'crisp', codec: 'hevc' });
    expect(savedBody({ preset: 'fast' }, null)).toEqual({ preset: 'fast', codec: 'h264' });
    const adv: StreamReq = { preset: 'custom', width: 1280, height: 720, fps: 30, bitrateKbps: 8000, codec: 'h264' };
    expect(savedBody(adv, 'hevc')).toEqual({ preset: 'custom', codec: 'h264', width: 1280, height: 720, fps: 30, bitrateKbps: 8000 });
  });
});

describe('prefsChip', () => {
  it('names the saved quality', () => {
    expect(prefsChip(P({ preset: 'crisp', codec: 'h264' }))).toBe('Crisp');
    expect(prefsChip(P({ preset: 'balanced', codec: 'h264' }))).toBe('Balanced');
    expect(prefsChip(P({ preset: 'crisp', codec: 'hevc' }))).toBe('HEVC · 40 Mbps');
    expect(prefsChip(P({ preset: 'fast', codec: 'hevc' }))).toBe('HEVC · 10 Mbps');
    expect(prefsChip(P({ preset: 'custom', codec: 'h264', bitrateKbps: 8000 }))).toBe('Custom · 8 Mbps');
    expect(prefsChip(P({ preset: 'custom', codec: 'hevc', bitrateKbps: 12500 }))).toBe('HEVC · 12.5 Mbps');
    expect(prefsChip(P({ preset: 'custom', codec: 'h264' }))).toBe('Custom · 20 Mbps');
  });
});

describe('prefSaver', () => {
  const rig = () => {
    const put = vi.fn(async (_id: number, b: { preset: string }) => P({ preset: b.preset as 'fast' }));
    const saved: [number, GamePrefs][] = [];
    const s = prefSaver({ put, onSaved: (id, p) => saved.push([id, p]) });
    return { put, saved, s };
  };

  it('saves once, 1 s after the last change, and the last one wins', async () => {
    vi.useFakeTimers();
    const { put, saved, s } = rig();
    s.save(7, { preset: 'fast', codec: 'h264' });
    await vi.advanceTimersByTimeAsync(600);
    s.save(7, { preset: 'balanced', codec: 'h264' });
    await vi.advanceTimersByTimeAsync(600);
    expect(put).not.toHaveBeenCalled(); // the second change restarted the wait
    s.save(7, { preset: 'crisp', codec: 'hevc' });
    await vi.advanceTimersByTimeAsync(999);
    expect(put).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(put).toHaveBeenCalledTimes(1);
    expect(put).toHaveBeenCalledWith(7, { preset: 'crisp', codec: 'hevc' });
    expect(saved.map((x) => x[0])).toEqual([7]);
    await vi.advanceTimersByTimeAsync(5000);
    expect(put).toHaveBeenCalledTimes(1);
  });

  it('a change for another game saves the pending one at once', async () => {
    vi.useFakeTimers();
    const { put, s } = rig();
    s.save(1, { preset: 'fast', codec: 'h264' });
    s.save(2, { preset: 'crisp', codec: 'h264' });
    expect(put).toHaveBeenCalledWith(1, { preset: 'fast', codec: 'h264' });
    await vi.advanceTimersByTimeAsync(1000);
    expect(put).toHaveBeenCalledWith(2, { preset: 'crisp', codec: 'h264' });
  });

  it('flush saves a pending change now (leaving the page); cancel drops it (forget)', async () => {
    vi.useFakeTimers();
    const { put, s } = rig();
    s.save(1, { preset: 'fast', codec: 'h264' });
    s.flush();
    expect(put).toHaveBeenCalledTimes(1);
    s.save(1, { preset: 'crisp', codec: 'h264' });
    s.cancel(1);
    await vi.advanceTimersByTimeAsync(2000);
    s.flush();
    expect(put).toHaveBeenCalledTimes(1);
  });

  it('a failed save is quiet and does not touch the store', async () => {
    vi.useFakeTimers();
    const onSaved = vi.fn();
    const s = prefSaver({ put: async () => Promise.reject(new Error('503')), onSaved });
    s.save(1, { preset: 'fast', codec: 'h264' });
    await vi.advanceTimersByTimeAsync(1000);
    expect(onSaved).not.toHaveBeenCalled();
  });
});

describe('forget vs an in-flight save', () => {
  it('forget waits for the PUT, then DELETEs, and the late PUT does not re-add the game', async () => {
    const order: string[] = [];
    let release!: () => void;
    const put = vi.fn(async () => {
      order.push('put-start');
      await new Promise<void>((r) => (release = r));
      order.push('put-end');
      return P();
    });
    const s = prefSaver({ put, onSaved: (id, p) => prefs.update((m) => ({ ...m, [id]: p })) });
    s.save(5, { preset: 'fast', codec: 'h264' });
    s.flush(); // PUT is now in flight
    const del = vi.fn(async () => void order.push('delete'));
    const forgot = forgetPrefs(5, { del, cancel: s.cancel, settled: s.settled });
    await Promise.resolve();
    expect(del).not.toHaveBeenCalled(); // still waiting for the PUT
    release();
    await forgot;
    expect(order).toEqual(['put-start', 'put-end', 'delete']);
    expect(get(prefs)['5']).toBeUndefined();
  });
});

describe('forgetPrefs', () => {
  it('deletes on hostd, then drops it from the store; keeps it if hostd fails', async () => {
    prefs.set({ '5': P(), '6': P() });
    const del = vi.fn(async () => {});
    const cancel = vi.fn();
    await forgetPrefs(5, { del, cancel, settled: async () => {} });
    expect(del).toHaveBeenCalledWith(5);
    expect(cancel).toHaveBeenCalledWith(5);
    expect(Object.keys(get(prefs))).toEqual(['6']);
    await expect(forgetPrefs(6, { del: async () => Promise.reject(new Error('x')), cancel, settled: async () => {} })).rejects.toThrow('x');
    expect(Object.keys(get(prefs))).toEqual(['6']);
  });
});

describe('startGame (Library, Resume card, Stream Start)', () => {
  const rig = (hevc = true) => {
    const startStream = vi.fn(async (_r: StreamReq) => ({}) as never);
    const notify = vi.fn();
    return { startStream, notify, d: { startStream, defaultPreset: () => 'fast' as const, hevcOk: async () => hevc } };
  };
  it("starts with that game's saved quality, else the default preset", async () => {
    prefs.set({ '10': P({ preset: 'crisp', codec: 'hevc' }) });
    const { startStream, notify, d } = rig();
    await startGame(10, notify, d);
    expect(startStream).toHaveBeenLastCalledWith({ preset: 'crisp', codec: 'hevc', appid: 10 });
    await startGame(11, notify, d);
    expect(startStream).toHaveBeenLastCalledWith({ preset: 'fast', codec: 'h264', appid: 11 });
    await startGame(undefined, notify, d);
    expect(startStream).toHaveBeenLastCalledWith({ preset: 'fast', codec: 'h264' }); // Desktop: no appid
    expect(notify).not.toHaveBeenCalled();
  });
  it('a saved HEVC falls back to H.264 with a notice when the browser cannot decode it; the entry stays', async () => {
    prefs.set({ '10': P({ preset: 'crisp', codec: 'hevc' }) });
    const { startStream, notify, d } = rig(false);
    await startGame(10, notify, d);
    expect(startStream).toHaveBeenLastCalledWith({ preset: 'crisp', codec: 'h264', appid: 10 });
    expect(notify).toHaveBeenCalledWith('This browser cannot decode HEVC, starting in H.264');
    expect(get(prefs)['10'].codec).toBe('hevc');
  });
});

describe('watchPrefs failure', () => {
  it('a failed fetch is retried on the next focus, not held off for a minute', async () => {
    vi.resetModules();
    const m = await import('./prefs');
    const f = vi.fn(async () => new Response('no', { status: 500 }));
    vi.stubGlobal('fetch', f);
    const win = new EventTarget();
    const stop = m.watchPrefs(win);
    await new Promise((r) => setTimeout(r));
    win.dispatchEvent(new Event('focus'));
    expect(f).toHaveBeenCalledTimes(2);
    stop();
  });
});

describe('refreshPrefs', () => {
  it('shares one GET between callers and fills homeOrder ([] when hostd has none)', async () => {
    vi.resetModules();
    const m = await import('./prefs');
    const f = vi.fn(async () => new Response(JSON.stringify({ prefs: {}, homeOrder: ['recent', 'host'] })));
    vi.stubGlobal('fetch', f);
    expect(get(m.homeOrder)).toBeUndefined();
    await Promise.all([m.refreshPrefs(), m.refreshPrefs()]);
    expect(f).toHaveBeenCalledTimes(1);
    expect(get(m.homeOrder)).toEqual(['recent', 'host']);
    f.mockImplementation(async () => new Response(JSON.stringify({ prefs: {} })));
    await m.refreshPrefs();
    expect(f).toHaveBeenCalledTimes(2);
    expect(get(m.homeOrder)).toEqual([]);
  });
});

describe('putPrefs', () => {
  it('uses keepalive so a save can finish while the page unloads', async () => {
    const f = vi.fn(async () => new Response(JSON.stringify(P())));
    vi.stubGlobal('fetch', f);
    const { api } = await import('./api');
    await api.putPrefs(1, { preset: 'fast', codec: 'h264' });
    expect((f.mock.calls[0] as unknown as [string, RequestInit])[1].keepalive).toBe(true);
  });
});

describe('watchPrefs', () => {
  it('fetches once, refreshes on focus (at most once a minute) and fills the store', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const m = await import('./prefs');
    const f = vi.fn(async () => new Response(JSON.stringify({ prefs: { '3': P() } })));
    vi.stubGlobal('fetch', f);
    const win = new EventTarget();
    const stop = m.watchPrefs(win);
    await vi.advanceTimersByTimeAsync(0);
    expect(f).toHaveBeenCalledTimes(1);
    expect(Object.keys(get(m.prefs))).toEqual(['3']);
    win.dispatchEvent(new Event('focus'));
    expect(f).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(61_000);
    win.dispatchEvent(new Event('focus'));
    expect(f).toHaveBeenCalledTimes(2);
    stop();
  });
});
