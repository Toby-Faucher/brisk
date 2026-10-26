// Clipboard sync with the host and the file upload queue of the Stream page. ClipSync is the
// logic (fakes in tests); browserClipboard is the thin navigator.clipboard layer. Never logs
// clipboard contents.
import type { Clip, Saved } from '../../state/api';

/** Same cap as hostd: bigger clipboards are skipped both ways. */
export const CLIP_MAX = 16 << 20;
export const POLL_MS = 1000;

export type LocalClipboard = { read(): Promise<Clip | null>; write(c: Clip): Promise<void> };
export type ClipDeps = {
  local: LocalClipboard;
  api: { get(since: number | null): Promise<{ seq: number; clip: Clip | null }>; post(c: Clip): Promise<void> };
  /** The page has focus and is visible: browsers refuse clipboard access otherwise. */
  focused: () => boolean;
  /** Setting on and the stream live. */
  enabled: () => boolean;
  notify: (text: string, error?: boolean) => void;
  onstatus?: (s: 'ok' | 'denied') => void;
};

const size = (c: Clip) => (c.kind === 'text' ? new TextEncoder().encode(c.text).length : c.blob.size);

export async function clipHash(c: Clip): Promise<string> {
  const data = c.kind === 'text' ? new TextEncoder().encode(c.text) : new Uint8Array(await c.blob.arrayBuffer());
  const buf = new Uint8Array(data.length + 1);
  buf[0] = c.kind === 'text' ? 0 : 1;
  buf.set(data, 1);
  const d = new Uint8Array(await crypto.subtle.digest('SHA-256', buf));
  return Array.from(d, (b) => b.toString(16).padStart(2, '0')).join('');
}

const isDenied = (e: unknown) => (e as Error)?.name === 'NotAllowedError';

export class ClipSync {
  private seq: number | null = null; // hostd's clipboard seq we have seen; null = adopt the next one
  private last: string | null = null; // hash of what both sides hold now (loop prevention)
  private skipped: string | null = null; // oversized clip already toasted
  private pending: Clip | null = null; // bawn's clip not written yet (page lost focus)
  private denied = false;
  private chain: Promise<unknown> = Promise.resolve();
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(private d: ClipDeps) {}

  /** Stream is live: poll every second and send the laptop clipboard now. */
  start() {
    this.stop();
    this.timer = setInterval(() => void this.poll(), POLL_MS);
    void this.focus();
  }

  stop() {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  /** Window focus, visibility, pointer lock, stream start: send a laptop copy, else write what waited. */
  focus() {
    return this.serial(async () => {
      if (!this.d.enabled() || !this.d.focused()) return;
      const sent = await this.push();
      const p = this.pending;
      this.pending = null;
      if (!sent && p) await this.apply(p);
    });
  }

  /** bawn -> laptop, once. */
  poll() {
    return this.serial(async () => {
      if (!this.d.enabled()) return void (this.seq = null); // turned back on: bawn's old clipboard stays there
      if (!this.d.focused()) return;
      let r;
      try {
        r = await this.d.api.get(this.seq);
      } catch {
        return; // hostd away: the stream shows that already
      }
      const first = this.seq === null;
      this.seq = r.seq;
      if (!first && r.clip) await this.apply(r.clip);
    });
  }

  /** This page itself wrote the laptop clipboard (a screenshot): `written` resolves to what it
   *  wrote, or null if it did not. Adopted as synced, so it is not sent to the host. */
  adopt(written: Promise<Clip | null>) {
    return this.serial(async () => {
      const c = await written.catch(() => null);
      if (!c) return;
      this.last = await clipHash(c);
      try {
        const back = await this.d.local.read(); // Chrome may re-encode the PNG
        if (back) this.last = await clipHash(back);
      } catch {}
    });
  }

  private serial(f: () => Promise<void>): Promise<void> {
    const p = this.chain.then(f, f);
    this.chain = p.catch(() => {});
    return p;
  }

  private async push(): Promise<boolean> {
    let c: Clip | null;
    try {
      c = await this.d.local.read();
    } catch (e) {
      this.failed(e);
      return false;
    }
    this.ok();
    if (!c) return false;
    const h = await clipHash(c);
    if (h === this.last || this.tooBig(c, h)) return false;
    try {
      await this.d.api.post(c);
    } catch (e) {
      this.d.notify(`Couldn't send the clipboard: ${(e as Error).message}`, true);
      return false;
    }
    this.last = h;
    return true;
  }

  private async apply(c: Clip) {
    const h = await clipHash(c);
    if (h === this.last || this.tooBig(c, h)) return;
    if (!this.d.focused()) return void (this.pending = c);
    try {
      await this.d.local.write(c);
    } catch (e) {
      this.pending = c;
      return this.failed(e);
    }
    this.ok();
    this.last = h;
    // The browser may re-encode an image: what reads back is what must not be sent back.
    try {
      const back = await this.d.local.read();
      if (back) this.last = await clipHash(back);
    } catch {}
  }

  private tooBig(c: Clip, h: string) {
    if (size(c) <= CLIP_MAX) return false;
    if (this.skipped !== h) this.d.notify(`Clipboard ${c.kind === 'text' ? 'text' : 'image'} is over 16 MiB: not synced`, true);
    this.skipped = h;
    return true;
  }

  private failed(e: unknown) {
    // Unfocused: the browser refuses; focus() retries. Only a real refusal is reported.
    if (!this.d.focused() || !isDenied(e) || this.denied) return;
    this.denied = true;
    this.d.onstatus?.('denied');
    this.d.notify('Clipboard sync needs clipboard access: allow it in the address bar (site settings)', true);
  }

  private ok() {
    if (!this.denied) return;
    this.denied = false;
    this.d.onstatus?.('ok');
  }
}

/** navigator.clipboard as a LocalClipboard: PNG before text, empty text is nothing. */
export function browserClipboard(cb?: Clipboard): LocalClipboard {
  const nav = () => cb ?? navigator.clipboard;
  return {
    async read() {
      for (const it of await nav().read()) {
        if (it.types.includes('image/png')) return { kind: 'image', blob: await it.getType('image/png') };
        if (it.types.includes('text/plain')) {
          const text = await (await it.getType('text/plain')).text();
          return text ? { kind: 'text', text } : null;
        }
      }
      return null;
    },
    write: (c) => (c.kind === 'text' ? nav().writeText(c.text) : nav().write([new ClipboardItem({ 'image/png': c.blob })])),
  };
}

export type Upload = { name: string; loaded: number; total: number };

/** Files go up one at a time, in the order they were added. `progress(null, 0)` once all are done.
 * `cancel` (leaving the page) aborts the active one and drops the rest; hostd removes the partial. */
export function uploadQueue(
  send: (f: File, progress: (loaded: number) => void, signal: AbortSignal) => Promise<Saved>,
  on: { progress(u: Upload | null, queued: number): void; done(name: string, path: string): void; error(name: string, msg: string): void },
) {
  const q: File[] = [];
  let running = false;
  let ctl = new AbortController();
  async function run() {
    running = true;
    while (q.length) {
      const f = q.shift()!;
      const report = (loaded: number) => on.progress({ name: f.name, loaded, total: f.size }, q.length);
      report(0);
      const signal = ctl.signal;
      try {
        on.done(f.name, (await send(f, report, signal)).path);
      } catch (e) {
        if (!signal.aborted) on.error(f.name, (e as Error).message);
      }
    }
    running = false;
    on.progress(null, 0);
  }
  return {
    add(files: Iterable<File>) {
      q.push(...files);
      if (!running) void run();
    },
    cancel() {
      q.length = 0;
      ctl.abort();
      ctl = new AbortController();
    },
  };
}
