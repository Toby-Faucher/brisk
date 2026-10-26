// Session history: the page's 500 ms delay/fps/bitrate samples, averaged to one point per 5 s and
// PUT to hostd (`/api/sessions/<started>/series`, merged there by t) every 30 s and on close.
import type { SeriesPoint } from '../../state/api';

/** `[t seconds since the session started, delay ms, fps, Mbit/s]`, as hostd stores it. */
export type Point = SeriesPoint;
export const STEP_S = 5;
export const UPLOAD_MS = 30_000;

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

/** Collects samples and uploads them in the background. A failed PUT is dropped quietly. */
export class SeriesUploader {
  private id: number | null = null;
  private pending: Point[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private put: (id: number, points: Point[]) => Promise<unknown>,
    private now = () => Date.now(),
  ) {}

  /** `id` = the session's `started` (hostd's clock, unix ms); t is clamped at 0 for a laptop clock behind it. */
  add(id: number, delay: number, fps: number, mbps: number) {
    if (this.id !== id) {
      this.close();
      this.id = id;
    }
    this.pending.push([Math.max(0, (this.now() - id) / 1000), delay, fps, mbps]);
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
    if (id === null || !this.pending.length) return;
    const points = downsample(this.pending);
    // The current bucket goes now and again next time, complete (hostd keeps the later one).
    const last = points[points.length - 1][0];
    this.pending = final ? [] : this.pending.filter((p) => Math.floor(p[0] / STEP_S) * STEP_S === last);
    try {
      this.put(id, points).catch(() => {});
    } catch {}
  }
}
