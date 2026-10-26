import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { StreamInfo } from '../../state/api';
import { RETRY_MS, StreamSession, pollLive, type Deps } from './session';

const LIVE: StreamInfo = { state: 'live', preset: 'balanced', codec: 'h264', appid: 42, wt: 'https://h:4433/a', hash: 'aa' };

function fakeTransport(ok = true) {
  let close!: () => void;
  const closed = new Promise<void>((r) => (close = r));
  return {
    ready: ok ? Promise.resolve() : Promise.reject(new Error('refused')),
    closed,
    close: vi.fn(() => close()),
    drop: () => close(), // the far end went away
  };
}

function fakeDeps(over: Partial<Deps> = {}) {
  const transports: ReturnType<typeof fakeTransport>[] = [];
  const release = vi.fn();
  const ctxs: { state: string; resume: any; close: any }[] = [];
  const deps: Deps = {
    api: { stream: vi.fn(async () => ({ ...LIVE, wt: 'https://h:4433/b' })), startStream: vi.fn(async () => ({ ...LIVE, wt: 'https://h:4433/new' })) },
    open: vi.fn(() => {
      const t = fakeTransport(transports.length === 0 || !failAfterFirst.on);
      transports.push(t);
      return t;
    }),
    audioContext: () => {
      const c = { state: 'running', resume: vi.fn(async () => void (c.state = 'running')), close: vi.fn(async () => {}) };
      ctxs.push(c);
      return c;
    },
    startVideo: () => ({ stats: { framesPresented: 0 } as any, done: new Promise(() => {}) }),
    startAudio: async () => ({ stats: {} as any, done: new Promise(() => {}) }),
    startInput: async () => ({ stats: {} as any, release }),
    canvas: () => ({}) as HTMLCanvasElement,
    notify: vi.fn(),
    ...over,
  };
  const failAfterFirst = { on: true };
  return { deps, transports, release, ctxs, failAfterFirst };
}

const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

describe('StreamSession', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('retries_with_backoff_then_fails_after_5', async () => {
    const f = fakeDeps();
    const s = new StreamSession(f.deps);
    await s.connect(LIVE);
    expect(get(s.state)).toBe('live');
    f.transports[0].drop();
    await flush();
    expect(get(s.state)).toBe('reconnecting');
    expect(f.deps.notify).toHaveBeenCalled();
    for (const [i, ms] of RETRY_MS.entries()) {
      await vi.advanceTimersByTimeAsync(ms - 1);
      expect(f.deps.open).toHaveBeenCalledTimes(1 + i);
      await vi.advanceTimersByTimeAsync(1);
      await flush();
      expect(f.deps.open).toHaveBeenCalledTimes(2 + i);
    }
    // First retry reuses the same URL; later ones ask hostd for the current one.
    expect((f.deps.open as any).mock.calls[1][0]).toBe(LIVE.wt);
    expect((f.deps.open as any).mock.calls[2][0]).toBe('https://h:4433/b');
    expect(f.deps.api.stream).toHaveBeenCalledTimes(4);
    expect(get(s.state)).toBe('failed');
    await vi.advanceTimersByTimeAsync(60_000);
    expect(f.deps.open).toHaveBeenCalledTimes(6);
  });

  it('reconnects_and_goes_live_again', async () => {
    const f = fakeDeps();
    f.failAfterFirst.on = false;
    const s = new StreamSession(f.deps);
    await s.connect(LIVE);
    f.transports[0].drop();
    await vi.advanceTimersByTimeAsync(RETRY_MS[0]);
    await flush();
    expect(get(s.state)).toBe('live');
    expect(f.deps.notify).toHaveBeenLastCalledWith('Reconnected');
  });

  it('every_sample_goes_to_the_history_hook_with_the_session_id', async () => {
    const sample = vi.fn();
    const f = fakeDeps({ sample });
    const s = new StreamSession(f.deps);
    await s.connect({ ...LIVE, started: 1234 });
    await vi.advanceTimersByTimeAsync(1000);
    expect(sample).toHaveBeenCalledTimes(2);
    expect(sample).toHaveBeenLastCalledWith(1234, 0, 0, 0);
    s.disconnect();
    // an older hostd without `started`: nothing to record
    const g = fakeDeps({ sample });
    const t = new StreamSession(g.deps);
    await t.connect(LIVE);
    await vi.advanceTimersByTimeAsync(1000);
    expect(sample).toHaveBeenCalledTimes(2);
    t.disconnect();
  });

  it('user_disconnect_does_not_retry', async () => {
    const f = fakeDeps();
    const s = new StreamSession(f.deps);
    await s.connect(LIVE);
    s.disconnect();
    await flush();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(f.deps.open).toHaveBeenCalledTimes(1);
    expect(f.transports[0].close).toHaveBeenCalledTimes(1);
  });

  it('change_quality_keeps_the_live_codec_unless_asked', async () => {
    const f = fakeDeps();
    f.failAfterFirst.on = false;
    const s = new StreamSession(f.deps);
    await s.connect({ ...LIVE, codec: 'hevc' });
    await s.changeQuality({ preset: 'fast' });
    expect(f.deps.api.startStream).toHaveBeenLastCalledWith({ preset: 'fast', codec: 'hevc' });
    await s.changeQuality({ preset: 'custom', width: 1280, height: 720, codec: 'h264' });
    expect(f.deps.api.startStream).toHaveBeenLastCalledWith({ preset: 'custom', width: 1280, height: 720, codec: 'h264' });
  });

  it('change_quality_without_a_live_session_sends_no_codec', async () => {
    const f = fakeDeps();
    const s = new StreamSession(f.deps);
    await s.changeQuality({ preset: 'fast' });
    expect(f.deps.api.startStream).toHaveBeenCalledWith({ preset: 'fast' });
  });

  it('change_quality_restarts_with_no_appid', async () => {
    const f = fakeDeps();
    f.failAfterFirst.on = false;
    const s = new StreamSession(f.deps);
    await s.connect(LIVE);
    await s.changeQuality({ preset: 'crisp', appid: 42 });
    expect(f.deps.api.startStream).toHaveBeenCalledWith({ preset: 'crisp', codec: 'h264' });
    expect(f.transports[0].close).toHaveBeenCalledTimes(1);
    expect((f.deps.open as any).mock.calls[1][0]).toBe('https://h:4433/new');
    expect(get(s.state)).toBe('live');
    await vi.advanceTimersByTimeAsync(60_000);
    expect(f.deps.open).toHaveBeenCalledTimes(2); // the old transport closing is ours: no retry
  });

  it('leaving_page_disconnects_and_releases_keys', async () => {
    const f = fakeDeps();
    const s = new StreamSession(f.deps);
    await s.connect(LIVE);
    await flush();
    s.disconnect();
    s.disconnect(); // a second teardown (destroy after the Disconnect button) is a no-op
    expect(f.release).toHaveBeenCalledTimes(1);
    expect(f.transports[0].close).toHaveBeenCalledTimes(1);
    expect(f.ctxs[0].close).toHaveBeenCalledTimes(1);
  });

  it('pads_show_for_the_live_connection_only', async () => {
    let onpads!: (p: { slot: number; name: string }[]) => void;
    const f = fakeDeps({ startInput: async (_t, _c, cb) => ((onpads = cb), { release: vi.fn() }) });
    const s = new StreamSession(f.deps);
    await s.connect(LIVE);
    await flush();
    onpads([{ slot: 0, name: '8BitDo' }]);
    expect(get(s.pads)).toEqual([{ slot: 0, name: '8BitDo' }]);
    const old = onpads;
    f.transports[0].drop();
    await flush();
    expect(get(s.pads)).toEqual([]);
    old([{ slot: 1, name: 'late' }]); // the dead connection's sender: ignored
    expect(get(s.pads)).toEqual([]);
    s.disconnect();
  });

  it('suspended_audiocontext_sets_needs_sound_prompt', async () => {
    const f = fakeDeps();
    const base = f.deps.audioContext;
    f.deps.audioContext = () => Object.assign(base(), { state: 'suspended' });
    const s = new StreamSession(f.deps);
    await s.connect(LIVE);
    expect(get(s.needsSound)).toBe(true);
    await s.resumeSound();
    expect(f.ctxs[0].resume).toHaveBeenCalled();
    expect(get(s.needsSound)).toBe(false);
  });

  it('disconnect_before_late_connect_creates_nothing', async () => {
    const f = fakeDeps();
    const s = new StreamSession(f.deps);
    s.disconnect(); // the page unmounted while boot()/start() was still awaiting hostd
    await s.connect(LIVE);
    await s.changeQuality({ preset: 'crisp' });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(f.deps.open).not.toHaveBeenCalled();
    expect(f.ctxs).toHaveLength(0);
  });

  it('disconnect_while_opening_closes_the_new_transport_and_audio', async () => {
    let ready!: () => void;
    const t = { ready: new Promise<void>((r) => (ready = r)), closed: new Promise(() => {}), close: vi.fn() };
    const startVideo = vi.fn();
    const f = fakeDeps({ open: vi.fn(() => t), startVideo });
    const s = new StreamSession(f.deps);
    const p = s.connect(LIVE);
    s.disconnect();
    ready();
    await p;
    expect(t.close).toHaveBeenCalledTimes(1);
    expect(f.ctxs[0].close).toHaveBeenCalledTimes(1);
    expect(startVideo).not.toHaveBeenCalled();
  });

  it('unmount_during_starting_poll_never_connects', async () => {
    let alive = true;
    const api = { stream: vi.fn(async () => ({ ...LIVE, state: 'starting' as const })), startStream: vi.fn() };
    const p = pollLive(api, () => alive);
    await vi.advanceTimersByTimeAsync(1200);
    alive = false;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(await p).toBeNull();
    expect(api.stream.mock.calls.length).toBeLessThanOrEqual(4);
  });

  it('poll_returns_live_once_started', async () => {
    let n = 0;
    const api = { stream: vi.fn(async () => ({ ...LIVE, state: n++ < 2 ? ('starting' as const) : ('live' as const) })), startStream: vi.fn() };
    const p = pollLive(api, () => true);
    await vi.advanceTimersByTimeAsync(2000);
    expect((await p)?.state).toBe('live');
  });
});
