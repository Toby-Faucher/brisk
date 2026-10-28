// Pure helpers behind LatencyLayers.svelte (no DOM, no stores).

/** One ping round: ms (null = no answer). Same shape as state/machines.ts; kept structural here so ui/ never imports state/. */
export type PingSample = { t: number; direct: number | null; hub: number | null };
export type Layer = 'direct' | 'hub';
export type Stats = { now: number | null; min: number | null; avg: number | null; max: number | null; lossPct: number };
export type Pt = { x: number; y: number | null };

/** The window the chart shows: 60 samples, one every 3 s. */
export const SPAN_MS = 59 * 3000;

export function stats(samples: PingSample[], key: Layer): Stats {
  const v = samples.map((s) => s[key]);
  const ok = v.filter((n): n is number => n !== null);
  return {
    now: v.length ? v[v.length - 1] : null,
    min: ok.length ? Math.min(...ok) : null,
    avg: ok.length ? ok.reduce((a, b) => a + b, 0) / ok.length : null,
    max: ok.length ? Math.max(...ok) : null,
    lossPct: v.length ? ((v.length - ok.length) / v.length) * 100 : 0,
  };
}

/** The next 1-2-5 step at or above `ms` (floor 10), so the top gridline is a round number. */
export function niceMax(ms: number): number {
  if (!Number.isFinite(ms) || ms <= 10) return 10;
  const p = 10 ** Math.floor(Math.log10(ms));
  const m = ms / p;
  return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 5 ? 5 : 10) * p;
}

/** Time -> x. The newest sample sits at `w`; older ones to its left, one SPAN_MS wide in all. */
export const timeX = (t: number, tNow: number, w: number, span = SPAN_MS) => w - ((tNow - t) / span) * w;

const f = (n: number) => +n.toFixed(2);

/** Runs of consecutive non-null points: a lost sample splits the line, it is never bridged. */
export function segments(pts: Pt[]): { x: number; y: number }[][] {
  const out: { x: number; y: number }[][] = [];
  let cur: { x: number; y: number }[] = [];
  for (const p of pts) {
    if (p.y === null) {
      if (cur.length) out.push(cur);
      cur = [];
    } else cur.push({ x: p.x, y: p.y });
  }
  if (cur.length) out.push(cur);
  return out;
}

/** Fritsch-Carlson tangents: a monotone cubic never overshoots between samples (no fake dips below 0 or spikes). */
function tangents(s: { x: number; y: number }[]): number[] {
  const n = s.length;
  const d = s.slice(1).map((p, i) => (p.y - s[i].y) / (p.x - s[i].x || 1));
  const m = s.map((_, i) => (i === 0 ? d[0] : i === n - 1 ? d[n - 2] : d[i - 1] * d[i] <= 0 ? 0 : (d[i - 1] + d[i]) / 2));
  d.forEach((k, i) => {
    if (k === 0) m[i] = m[i + 1] = 0;
    else {
      const a = m[i] / k, b = m[i + 1] / k, h = Math.hypot(a, b);
      if (h > 3) m[i] = (3 * a * k) / h, m[i + 1] = (3 * b * k) / h;
    }
  });
  return m;
}

/** Smooth edge of one run, starting with M (a lone point is a zero-length stub: round caps draw it as a dot). */
function curve(s: { x: number; y: number }[]): string {
  if (s.length === 1) return `M${f(s[0].x)} ${f(s[0].y)}h0`;
  const m = tangents(s);
  let d = `M${f(s[0].x)} ${f(s[0].y)}`;
  for (let i = 1; i < s.length; i++) {
    const dx = (s[i].x - s[i - 1].x) / 3;
    d += `C${f(s[i - 1].x + dx)} ${f(s[i - 1].y + m[i - 1] * dx)} ${f(s[i].x - dx)} ${f(s[i].y - m[i] * dx)} ${f(s[i].x)} ${f(s[i].y)}`;
  }
  return d;
}

/** Crisp top edge: one `M...C...` sub-path per run. */
export const edgePath = (pts: Pt[]) => segments(pts).map(curve).join('');

/** Filled area down to `base`: one closed sub-path per run of two or more points (a lone point has no area). */
export const areaPath = (pts: Pt[], base: number) =>
  segments(pts)
    .filter((s) => s.length > 1)
    .map((s) => `${curve(s)}L${f(s[s.length - 1].x)} ${f(base)}L${f(s[0].x)} ${f(base)}Z`)
    .join('');

/** x of every lost sample, for the baseline ticks. */
export const lossXs = (pts: Pt[]) => pts.filter((p) => p.y === null).map((p) => p.x);
