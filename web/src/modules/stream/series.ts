// Session history: the page's 500 ms delay/fps/bitrate samples, averaged to one point per 5 s and
// PUT to hostd (`/api/sessions/<started>/series`, merged there by t) every 30 s and on close.
import type { SeriesPoint } from '../../state/api';

/** `[t seconds since the session started, delay ms, fps, Mbit/s]`, as hostd stores it. */
export type Point = SeriesPoint;
export const STEP_S = 5;
export const UPLOAD_MS = 30_000;
/** Raw samples kept while hostd does not acknowledge them (~400 points, 33 min); the oldest go first. */
export const MAX_PENDING = 4000;

const bucket = (t: number) => Math.floor(t / STEP_S) * STEP_S;

/** Mean of each `step`-second bucket, t = the bucket's start; the last bucket may be partial. */
export function downsample(samples: Point[], step = STEP_S): Point[] {
  const out: Point[] = [];
  let sum: Point | null = null;
  let n = 0;
  const done = () => sum && out.push([sum[0], sum[1] / n, sum[2] / n, sum[3] / n]);
  for (const [t, d, f, m] of samples) {
    const b = Math.floor(t / step) * step;
    if (!sum || sum[0] !== b) {
      done();
      sum = [b, 0, 0, 0];
      n = 0;
    }
    sum[1] += d;
    sum[2] += f;
    sum[3] += m;
    n++;
  }
  done();
  return out;
}

/**
 * Collects samples and uploads them in the background, never awaited. Samples stay pending until a
 * PUT of them succeeds (a re-send is harmless: hostd merges by t). A 404 or 413 (unknown session, or
 * past hostd's 24 h cap) ends uploads for that session quietly; other failures retry next time.
 */
export class SeriesUploader {
  private id: number | null = null;
  private off = 0; // hostd's elapsed seconds at perf0
  private perf0 = 0;
  private minT = 0;
  private pending: Point[] = [];
  private busy = false;
  private stopped = false;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private put: (id: number, points: Point[]) => Promise<unknown>,
    private now = () => performance.now(),
  ) {}

  /**
   * `id` = the session's `started`; `elapsedS` = hostd's seconds since it started, as of now (only the
   * first sample of a session reads it: later t = that + this page's monotonic clock, so neither the
   * laptop's wall clock nor a reload of /stream mid-session can shift the series).
   */
  add(id: number, elapsedS: number | null, delay: number, fps: number, mbps: number) {
    if (this.id !== id) {
      this.close();
      this.id = id;
      this.off = Math.max(0, elapsedS ?? 0);
      this.perf0 = this.now();
      // A reload lands inside a bucket the earlier page already stored: start at the next one.
      this.minT = Math.ceil(this.off / STEP_S) * STEP_S;
      this.pending = [];
      this.busy = this.stopped = false;
    }
    const t = this.off + (this.now() - this.perf0) / 1000;
    if (this.stopped || t < this.minT) return;
    this.pending.push([t, delay, fps, mbps]);
    if (this.pending.length > MAX_PENDING) this.pending.splice(0, this.pending.length - MAX_PENDING);
    this.timer ??= setInterval(() => this.flush(false), UPLOAD_MS);
  }

  /** Disconnect, leaving /stream or pagehide: sends the partial bucket too and stops the timer. */
  close() {
    this.flush(true);
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
  }

  private flush(final: boolean) {
    const id = this.id;
    if (id === null || this.stopped || !this.pending.length || (this.busy && !final)) return;
    const batch = this.pending.slice();
    const points = downsample(batch);
    const last = points[points.length - 1][0];
    this.busy = true;
    const done = (e?: unknown) => {
      if (id !== this.id) return;
      this.busy = false;
      const status = (e as { status?: number } | undefined)?.status;
      if (status === 404 || status === 413) {
        this.stopped = true;
        this.pending = [];
      } else if (!e) {
        // Acknowledged; the current bucket goes again next time, complete (hostd keeps the later one).
        const sent = new Set(batch);
        this.pending = this.pending.filter((p) => !sent.has(p) || (!final && bucket(p[0]) === last));
      }
    };
    try {
      this.put(id, points).then(
        () => done(),
        (e) => done(e ?? new Error('failed')),
      );
    } catch (e) {
      done(e);
    }
  }
}
