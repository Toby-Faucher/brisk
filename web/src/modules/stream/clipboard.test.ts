import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Clip } from '../../state/api';
import { CLIP_MAX, ClipSync, POLL_MS, browserClipboard, uploadQueue, type ClipDeps } from './clipboard';

const denied = () => Object.assign(new Error('Read permission denied.'), { name: 'NotAllowedError' });

/** A laptop clipboard plus a hostd that echoes nothing back, like the real one. */
function rig(over: Partial<ClipDeps> = {}) {
  const st = { local: null as Clip | null, focused: true, enabled: true, seq: 3, remote: null as Clip | null, readErr: null as Error | null, writeErr: null as Error | null };
  const posts: Clip[] = [];
  const writes: Clip[] = [];
  const notify = vi.fn();
  const onstatus = vi.fn();
  const deps: ClipDeps = {
    local: {
      read: vi.fn(async () => {
        if (st.readErr) throw st.readErr;
        return st.local;
      }),
      write: vi.fn(async (c: Clip) => {
        if (st.writeErr) throw st.writeErr;
        writes.push(c);
        st.local = c;
      }),
    },
    api: {
      get: vi.fn(async (since: number | null) => ({ seq: st.seq, clip: since !== null && since !== st.seq ? st.remote : null })),
      post: vi.fn(async (c: Clip) => void posts.push(c)),
    },
    focused: () => st.focused,
    enabled: () => st.enabled,
    notify,
    onstatus,
    ...over,
  };
  const bawnCopies = (c: Clip) => {
    st.remote = c;
    st.seq++;
  };
  return { st, posts, writes, notify, onstatus, deps, bawnCopies, sync: new ClipSync(deps) };
}

const text = (t: string): Clip => ({ kind: 'text', text: t });

describe('ClipSync', () => {
  it('laptop_copy_is_sent_once', async () => {
    const r = rig();
    r.st.local = text('hello');
    await r.sync.focus();
    await r.sync.focus();
    expect(r.posts).toEqual([text('hello')]);
    r.st.local = text('again');
    await r.sync.focus();
    expect(r.posts).toEqual([text('hello'), text('again')]);
  });

  it('first_poll_only_adopts_the_seq', async () => {
    const r = rig();
    r.st.remote = text('stale on bawn');
    await r.sync.poll();
    expect(r.writes).toEqual([]);
    expect(r.deps.api.get).toHaveBeenCalledWith(null);
  });

  it('bawn_copy_is_written_and_not_sent_back', async () => {
    const r = rig();
    await r.sync.poll();
    r.bawnCopies(text('from bawn'));
    await r.sync.poll();
    expect(r.writes).toEqual([text('from bawn')]);
    await r.sync.focus(); // laptop clipboard now holds what bawn sent: no echo
    expect(r.posts).toEqual([]);
    await r.sync.poll();
    expect(r.writes).toHaveLength(1);
  });

  it('what_we_sent_coming_back_is_not_rewritten', async () => {
    const r = rig();
    await r.sync.poll();
    r.st.local = text('mine');
    await r.sync.focus();
    r.bawnCopies(text('mine')); // even if hostd did echo it
    await r.sync.poll();
    expect(r.writes).toEqual([]);
  });

  it('image_reencoded_by_the_browser_is_not_sent_back', async () => {
    const r = rig();
    const png = new Blob([new Uint8Array([1, 2, 3])], { type: 'image/png' });
    const reencoded = new Blob([new Uint8Array([9, 9, 9, 9])], { type: 'image/png' });
    r.deps.local.write = vi.fn(async () => void (r.st.local = { kind: 'image', blob: reencoded }));
    await r.sync.poll();
    r.bawnCopies({ kind: 'image', blob: png });
    await r.sync.poll();
    await r.sync.focus();
    expect(r.posts).toEqual([]);
  });

  it('permission_denied_toasts_once_and_reports_status', async () => {
    const r = rig();
    r.st.readErr = denied();
    await r.sync.focus();
    await r.sync.focus();
    expect(r.notify).toHaveBeenCalledTimes(1);
    expect(r.onstatus).toHaveBeenLastCalledWith('denied');
    r.st.readErr = null;
    r.st.local = text('ok now');
    await r.sync.focus();
    expect(r.onstatus).toHaveBeenLastCalledWith('ok');
    expect(r.posts).toEqual([text('ok now')]);
  });

  it('unfocused_write_waits_for_focus', async () => {
    const r = rig();
    await r.sync.poll();
    r.bawnCopies(text('later'));
    r.st.writeErr = Object.assign(new Error('Document is not focused.'), { name: 'NotAllowedError' });
    r.st.focused = false;
    // Focus went away between the poll and the write.
    r.deps.focused = vi.fn().mockReturnValueOnce(true).mockReturnValue(false);
    await r.sync.poll();
    expect(r.writes).toEqual([]);
    expect(r.notify).not.toHaveBeenCalled();
    r.st.writeErr = null;
    r.deps.focused = () => true;
    await r.sync.focus();
    expect(r.writes).toEqual([text('later')]);
  });

  it('does_not_poll_while_unfocused', async () => {
    const r = rig();
    r.st.focused = false;
    await r.sync.poll();
    await r.sync.focus();
    expect(r.deps.api.get).not.toHaveBeenCalled();
    expect(r.deps.local.read).not.toHaveBeenCalled();
  });

  it('skips_oversized_images_both_ways', async () => {
    const r = rig();
    const big: Clip = { kind: 'image', blob: new Blob([new Uint8Array(CLIP_MAX + 1)], { type: 'image/png' }) };
    r.st.local = big;
    await r.sync.focus();
    await r.sync.focus();
    expect(r.posts).toEqual([]);
    expect(r.notify).toHaveBeenCalledTimes(1);
    await r.sync.poll();
    r.bawnCopies(big);
    await r.sync.poll();
    expect(r.writes).toEqual([]);
  });

  it('size_is_utf8_bytes', async () => {
    const r = rig();
    r.st.local = text('a'.repeat(CLIP_MAX / 2)); // 8 MiB of ASCII: under the cap
    await r.sync.focus();
    expect(r.posts).toHaveLength(1);
    r.st.local = text('é'.repeat(CLIP_MAX / 2 + 1)); // 2 bytes each: over
    await r.sync.focus();
    expect(r.posts).toHaveLength(1);
    expect(r.notify).toHaveBeenCalledTimes(1);
  });

  it('toggle_off_reads_and_sends_nothing', async () => {
    const r = rig();
    r.st.enabled = false;
    r.st.local = text('secret');
    r.bawnCopies(text('x'));
    await r.sync.focus();
    await r.sync.poll();
    await r.sync.poll();
    expect(r.deps.local.read).not.toHaveBeenCalled();
    expect(r.deps.local.write).not.toHaveBeenCalled();
    expect(r.deps.api.get).not.toHaveBeenCalled();
    expect(r.deps.api.post).not.toHaveBeenCalled();
    // Back on: bawn's old clipboard is not pulled over, the laptop's is sent.
    r.st.enabled = true;
    await r.sync.focus();
    await r.sync.poll();
    expect(r.writes).toEqual([]);
    expect(r.posts).toEqual([text('secret')]);
  });

  describe('timer', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it('polls_every_second_until_stopped', async () => {
      const r = rig();
      r.sync.start();
      await vi.advanceTimersByTimeAsync(POLL_MS * 3 + 10);
      const n = (r.deps.api.get as any).mock.calls.length;
      expect(n).toBeGreaterThanOrEqual(3);
      r.sync.stop();
      await vi.advanceTimersByTimeAsync(POLL_MS * 3);
      expect((r.deps.api.get as any).mock.calls.length).toBe(n);
    });
  });
});

describe('browserClipboard', () => {
  afterEach(() => vi.unstubAllGlobals());

  const item = (types: Record<string, Blob>) => ({ types: Object.keys(types), getType: async (t: string) => types[t] });

  it('reads_png_before_text_and_text_otherwise', async () => {
    const png = new Blob([new Uint8Array([1])], { type: 'image/png' });
    const nav = { read: vi.fn(async () => [item({ 'text/html': new Blob(['<img>']), 'image/png': png })]), write: vi.fn(), writeText: vi.fn() };
    const b = browserClipboard(nav as any);
    expect(await b.read()).toEqual({ kind: 'image', blob: png });
    nav.read.mockResolvedValue([item({ 'text/plain': new Blob(['hi']) })]);
    expect(await b.read()).toEqual({ kind: 'text', text: 'hi' });
    nav.read.mockResolvedValue([item({ 'text/plain': new Blob(['']) })]);
    expect(await b.read()).toBeNull();
    nav.read.mockResolvedValue([]);
    expect(await b.read()).toBeNull();
  });

  it('writes_text_and_png', async () => {
    class FakeItem {
      constructor(readonly items: Record<string, Blob>) {}
    }
    vi.stubGlobal('ClipboardItem', FakeItem);
    const nav = { read: vi.fn(), write: vi.fn(async () => {}), writeText: vi.fn(async () => {}) };
    const b = browserClipboard(nav as any);
    await b.write(text('t'));
    expect(nav.writeText).toHaveBeenCalledWith('t');
    const png = new Blob([new Uint8Array([1])], { type: 'image/png' });
    await b.write({ kind: 'image', blob: png });
    expect((nav.write.mock.calls[0] as any)[0][0].items).toEqual({ 'image/png': png });
  });

  it('denied_read_throws_through', async () => {
    const nav = { read: vi.fn(async () => Promise.reject(denied())), write: vi.fn(), writeText: vi.fn() };
    await expect(browserClipboard(nav as any).read()).rejects.toMatchObject({ name: 'NotAllowedError' });
  });
});

describe('uploadQueue', () => {
  const file = (name: string, size = 10) => ({ name, size }) as File;

  it('sends_one_at_a_time_in_order_with_progress', async () => {
    const order: string[] = [];
    const finish: Record<string, () => void> = {};
    const send = vi.fn((f: File, progress: (n: number) => void) => {
      order.push(`start ${f.name}`);
      progress(f.size / 2);
      return new Promise<{ path: string; size: number }>((ok) => {
        finish[f.name] = () => {
          order.push(`end ${f.name}`);
          ok({ path: `/dl/${f.name}`, size: f.size });
        };
      });
    });
    const progress = vi.fn();
    const done = vi.fn();
    const error = vi.fn();
    const q = uploadQueue(send, { progress, done, error });
    q.add([file('a'), file('b')]);
    q.add([file('c')]);
    await Promise.resolve();
    expect(send).toHaveBeenCalledTimes(1);
    expect(progress).toHaveBeenCalledWith({ name: 'a', loaded: 5, total: 10 }, 1); // b waits; c came later
    finish.a();
    await vi.waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    finish.b();
    await vi.waitFor(() => expect(send).toHaveBeenCalledTimes(3));
    finish.c();
    await vi.waitFor(() => expect(progress).toHaveBeenLastCalledWith(null, 0));
    expect(order).toEqual(['start a', 'end a', 'start b', 'end b', 'start c', 'end c']);
    expect(done.mock.calls).toEqual([['a', '/dl/a'], ['b', '/dl/b'], ['c', '/dl/c']]);
    expect(error).not.toHaveBeenCalled();
  });

  it('cancel_aborts_the_active_upload_and_drops_the_queue', async () => {
    const signals: AbortSignal[] = [];
    const send = vi.fn(
      (_f: File, _p: (n: number) => void, signal: AbortSignal) =>
        new Promise<{ path: string; size: number }>((_ok, no) => {
          signals.push(signal);
          signal.addEventListener('abort', () => no(new Error('cancelled')));
        }),
    );
    const on = { progress: vi.fn(), done: vi.fn(), error: vi.fn() };
    const q = uploadQueue(send, on);
    q.add([file('a'), file('b')]);
    q.cancel();
    await vi.waitFor(() => expect(on.progress).toHaveBeenLastCalledWith(null, 0));
    expect(signals[0].aborted).toBe(true);
    expect(send).toHaveBeenCalledTimes(1);
    expect(on.error).not.toHaveBeenCalled();
  });

  it('an_error_does_not_stop_the_queue', async () => {
    const send = vi.fn(async (f: File) => {
      if (f.name === 'bad') throw new Error('not enough free disk space on bawn');
      return { path: `/dl/${f.name}`, size: 1 };
    });
    const done = vi.fn();
    const error = vi.fn();
    const progress = vi.fn();
    uploadQueue(send, { progress, done, error }).add([file('bad'), file('good')]);
    await vi.waitFor(() => expect(done).toHaveBeenCalledWith('good', '/dl/good'));
    expect(error).toHaveBeenCalledWith('bad', 'not enough free disk space on bawn');
  });
});
