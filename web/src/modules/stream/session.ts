// One stream session: connects the existing video/audio/input pipes (lib/*.ts) to the bridge's
// WebTransport, reconnects silently on a drop, restarts the bridge for a quality change, and
// samples stats for the ghost bar and the stats card. Everything browser-side is injected (Deps).
import { writable } from 'svelte/store';
import type { StreamInfo, StreamReq } from '../../state/api';
import type { VideoStats } from '../../lib/video';

export type SessionState = 'connecting' | 'live' | 'reconnecting' | 'failed';
/** Waits before each reconnect try after a drop; then 'failed'. */
export const RETRY_MS = [500, 1000, 2000, 4000, 8000];
/** A connection that lived this long earns a fresh set of retries (a flapping one does not). */
export const STABLE_MS = 5000;
const SAMPLE_MS = 500;
const POINTS = 60;

export type StatsView = {
  delay: number[]; // ms, estimate: see sample()
  fps: number[];
  bitrate: number[]; // Mbit/s, everything on the connection
  rttMs: number | null;
  width: number;
  height: number;
  codec: string | null;
  hw: string | null;
};

type Transport = {
  ready: Promise<unknown>;
  closed: Promise<unknown>;
  close(): void;
  getStats?(): Promise<{ bytesReceived?: number; smoothedRtt?: number }>;
};
type Gain = { gain: { value: number }; connect(n: unknown): unknown };
type Ctx = { state: string; resume(): Promise<unknown>; close(): Promise<unknown>; destination?: unknown; createGain?(): Gain };
type Canvas = HTMLCanvasElement;

export interface Deps {
  api: { stream(): Promise<StreamInfo>; startStream(r: StreamReq): Promise<StreamInfo> };
  open(url: string, hash: string): Transport;
  audioContext(): Ctx;
  startVideo(t: Transport, c: Canvas): { stats: VideoStats; done: Promise<void> };
  startAudio(t: Transport, ctx: Ctx, out?: Gain): Promise<unknown>;
  startInput(t: Transport, c: Canvas): Promise<{ release(): void }>;
  /** A fresh canvas per connection, so the old connection's input listeners see a dead element. */
  canvas(): Canvas;
  notify(text: string, error?: boolean): void;
}

type Conn = { t: Transport; ctx: Ctx; gain?: Gain; video?: { stats: VideoStats }; input?: { release(): void }; ours: boolean; liveAt: number };

/** hostd's stream, waiting up to 10 s while it is 'starting'; null as soon as `alive()` turns false. */
export async function pollLive(api: Deps['api'], alive: () => boolean): Promise<StreamInfo | null> {
  let info = await api.stream();
  for (let i = 0; alive() && info.state === 'starting' && i < 20; i++) {
    await new Promise((r) => setTimeout(r, 500));
    if (!alive()) return null;
    info = await api.stream();
  }
  return alive() ? info : null;
}

const live = (i: StreamInfo | null): i is StreamInfo & { wt: string; hash: string } => !!i && i.state === 'live' && !!i.wt && !!i.hash;

export class StreamSession {
  readonly state = writable<SessionState>('connecting');
  readonly stats = writable<StatsView>(empty());
  readonly needsSound = writable(false);
  readonly canvasEl = writable<Canvas | null>(null);
  info: StreamInfo | null = null;
  private conn: Conn | null = null;
  private closed = false; // after disconnect() nothing connects again (the page is gone)
  private gen = 0; // bumped by every connect/changeQuality/disconnect: stale async work checks it
  private tries = 0;
  private timer: ReturnType<typeof setInterval> | null = null;
  private volume = 1;

  constructor(private d: Deps) {}

  /** Connect to a live stream. Resolves once connected or once it gave up. */
  async connect(info: StreamInfo) {
    if (this.closed) return;
    const gen = ++this.gen;
    this.end();
    this.tries = 0;
    this.state.set('connecting');
    if (!live(info)) return this.state.set('failed');
    this.info = info;
    if (!(await this.open(info, gen)) && gen === this.gen) await this.retry(gen);
  }

  /** New launch parameters: hostd restarts the bridge (same game, so no appid) and we reconnect. */
  async changeQuality(req: StreamReq) {
    const gen = ++this.gen;
    this.end();
    this.state.set('connecting');
    const { appid: _, ...rest } = req;
    let info: StreamInfo;
    try {
      info = await this.d.api.startStream(rest);
    } catch (e) {
      if (gen === this.gen) {
        this.d.notify(`Couldn't change quality: ${(e as Error).message}`, true);
        this.state.set('failed');
      }
      return;
    }
    if (gen === this.gen) await this.connect(info);
  }

  /** User disconnect or leaving the page: release keys, close everything, no retry, no later connect. */
  disconnect() {
    this.closed = true;
    this.gen++;
    this.end();
  }

  /** 0..1, kept across reconnects; 0 = muted. */
  setVolume(v: number) {
    this.volume = v;
    if (this.conn?.gain) this.conn.gain.gain.value = v;
  }

  async resumeSound() {
    await this.conn?.ctx.resume();
    this.needsSound.set(this.conn?.ctx.state === 'suspended');
  }

  private async open(info: StreamInfo & { wt: string; hash: string }, gen: number): Promise<boolean> {
    // The AudioContext first, synchronously: inside a click it gets that click's activation.
    const ctx = this.d.audioContext();
    let t: Transport;
    try {
      t = this.d.open(info.wt, info.hash);
      await t.ready;
    } catch {
      void ctx.close().catch(() => {});
      return false;
    }
    if (gen !== this.gen) {
      t.close();
      void ctx.close().catch(() => {});
      return true;
    }
    const conn: Conn = { t, ctx, ours: false, liveAt: Date.now() };
    this.conn = conn;
    const canvas = this.d.canvas();
    this.canvasEl.set(canvas);
    conn.video = this.d.startVideo(t, canvas);
    this.d.startInput(t, canvas).then((i) => (conn.input = i), () => {});
    conn.gain = ctx.createGain?.();
    if (conn.gain) {
      conn.gain.gain.value = this.volume;
      conn.gain.connect(ctx.destination);
    }
    this.d.startAudio(t, ctx, conn.gain).catch(() => {});
    this.needsSound.set(ctx.state === 'suspended');
    this.stats.set(empty());
    this.startSampling(conn);
    this.state.set('live');
    t.closed.then(
      () => this.dropped(conn),
      () => this.dropped(conn),
    );
    return true;
  }

  private dropped(conn: Conn) {
    if (conn.ours || conn !== this.conn) return;
    this.end();
    if (Date.now() - conn.liveAt >= STABLE_MS) this.tries = 0;
    this.state.set('reconnecting');
    this.d.notify('Reconnecting… your keys are released');
    void this.retry(this.gen);
  }

  private async retry(gen: number) {
    while (this.tries < RETRY_MS.length) {
      const i = this.tries++;
      this.state.set('reconnecting');
      await new Promise((r) => setTimeout(r, RETRY_MS[i]));
      if (gen !== this.gen) return;
      let info: StreamInfo | null = this.info;
      if (i > 0) {
        info = await this.d.api.stream().catch(() => null);
        if (gen !== this.gen) return;
      }
      if (!live(info)) continue;
      this.info = info;
      if (await this.open(info, gen)) {
        if (gen === this.gen) this.d.notify('Reconnected');
        return;
      }
      if (gen !== this.gen) return;
    }
    this.state.set('failed');
  }

  /** Tears down the current connection, ours: release keys while the stream is still open. */
  private end() {
    const c = this.conn;
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    if (!c) return;
    this.conn = null;
    c.ours = true;
    c.input?.release();
    try {
      c.t.close();
    } catch {}
    void c.ctx.close().catch(() => {});
    this.needsSound.set(false);
  }

  private startSampling(conn: Conn) {
    let prev = { at: performance.now(), frames: 0, bytes: null as number | null };
    this.timer = setInterval(async () => {
      const s = conn.video!.stats;
      const at = performance.now();
      const ts = await conn.t.getStats?.().catch(() => undefined);
      if (conn !== this.conn) return;
      const dt = (at - prev.at) / 1000;
      const rtt = ts?.smoothedRtt ?? null;
      const bytes = s.bytesReceived ?? null; // Chrome's getStats().bytesReceived is 0
      const fps = dt > 0 ? (s.framesPresented - prev.frames) / dt : 0;
      const mbps = bytes !== null && prev.bytes !== null && dt > 0 ? ((bytes - prev.bytes) * 8) / dt / 1e6 : 0;
      prev = { at, frames: s.framesPresented, bytes };
      // Delay estimate: capture->bridge write (bawn clock) + half the RTT + queueing above the
      // best frame + decode + draw. Software timestamps only (docs/m0-baseline.md).
      const p = (w: { pct(n: number): number | null } | undefined) => w?.pct(50) ?? 0;
      const over = (s.queueMs?.pct(50) ?? 0) - (s.queueMs?.min() ?? 0);
      const delay = p(s.bridgeCaptureMs) + (rtt ?? 0) / 2 + over + p(s.decodeMs) + p(s.presentMs);
      this.stats.update((v) => ({
        delay: push(v.delay, delay),
        fps: push(v.fps, fps),
        bitrate: push(v.bitrate, mbps),
        rttMs: rtt,
        width: s.width,
        height: s.height,
        codec: s.codec,
        hw: s.hardwareAcceleration,
      }));
    }, SAMPLE_MS);
  }
}

function empty(): StatsView {
  return { delay: [], fps: [], bitrate: [], rttMs: null, width: 0, height: 0, codec: null, hw: null };
}

const push = (a: number[], x: number) => (a.length >= POINTS ? [...a.slice(1), x] : [...a, x]);
