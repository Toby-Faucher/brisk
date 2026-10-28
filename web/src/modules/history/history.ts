// View model of the History page: pure formatting, no fetching.
import type { SeriesPoint, SessionRec, SessionStats } from '../../state/api';
import { DASH } from '../home/home';
import { fold } from '../library/library';

export function listState(list: SessionRec[] | null, failed: boolean): 'loading' | 'error' | 'empty' | 'list' {
  if (!list) return failed ? 'error' : 'loading';
  return list.length ? 'list' : 'empty';
}

/** Sessions whose game name contains q (case, accents and punctuation ignored); a blank q keeps all. */
export function filterSessions<T extends { name: string }>(list: T[], q: string): T[] {
  const fq = fold(q);
  return fq ? list.filter((s) => fold(s.name).includes(fq)) : list;
}

const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const p2 = (n: number) => String(n).padStart(2, '0');

/** `Sun 25 Oct · 21:04`, local time. */
export function fmtWhen(ms: number): string {
  const d = new Date(ms);
  return `${DAYS[d.getDay()]} ${d.getDate()} ${MONTHS[d.getMonth()]} · ${p2(d.getHours())}:${p2(d.getMinutes())}`;
}

/** Seconds as `m:ss` or `h:mm:ss` (chart axis, bridge time). */
export function fmtClock(s: number): string {
  const t = Math.round(s);
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  return h ? `${h}:${p2(m)}:${p2(t % 60)}` : `${m}:${p2(t % 60)}`;
}

const PRESET = { fast: 'Fast', balanced: 'Balanced', crisp: 'Crisp', custom: 'Custom' } as const;

/** `Crisp · HEVC · 40 Mbps`; codec and bitrate only when the record has them. */
export function qualityLabel(r: SessionRec): string {
  const parts: string[] = [PRESET[r.preset] ?? r.preset];
  if (r.codec) parts.push(r.codec === 'hevc' ? 'HEVC' : 'H.264');
  if (r.bitrateKbps) parts.push(`${+(r.bitrateKbps / 1000).toFixed(1)} Mbps`);
  return parts.join(' · ');
}

/** SVG path of `v` across a `w`x`h` box (1 px inset top and bottom); '' below two values. */
export function sparkPath(v: number[], w: number, h: number): string {
  if (v.length < 2) return '';
  const lo = Math.min(...v);
  const span = Math.max(...v) - lo;
  const r = (x: number) => +x.toFixed(1);
  return v
    .map((y, i) => `${i ? 'L' : 'M'}${r((i / (v.length - 1)) * w)},${r(span ? 1 + (1 - (y - lo) / span) * (h - 2) : h / 2)}`)
    .join('');
}

/** `?session=<started>` of the History page. */
export function sessionParam(search: string): number | null {
  const v = new URLSearchParams(search).get('session');
  return v && /^\d+$/.test(v) ? Number(v) : null;
}

export type ChartRow = { t: number; delay: number; fps: number; mbps: number };
export const chartRows = (s: SeriesPoint[]): ChartRow[] => s.map(([t, delay, fps, mbps]) => ({ t, delay, fps, mbps }));

/** Min, mean and max of one column, and the t of its max (the charts' text alternative). */
export function summarize(
  rows: ChartRow[],
  k: Exclude<keyof ChartRow, 't'>,
): { min: number; avg: number; max: number; peakT: number } | null {
  if (!rows.length) return null;
  let [min, max, sum, peakT] = [Infinity, -Infinity, 0, 0];
  for (const r of rows) {
    sum += r[k];
    min = Math.min(min, r[k]);
    if (r[k] > max) [max, peakT] = [r[k], r.t];
  }
  return { min, avg: sum / rows.length, max, peakT };
}

const n = (v: number | null) => (v == null ? DASH : v.toLocaleString('en-US'));
const ms = (a: number | null, b: number | null) => (a == null && b == null ? DASH : `${a ?? DASH} / ${b ?? DASH} ms`);

/** `[label, value]` rows of the bridge's report (p50 / p99 for latencies); none without a report. */
export function statRows(s: SessionStats | null | undefined): [string, string][] {
  if (!s) return [];
  return [
    ['Bridge time', s.durationS == null ? DASH : fmtClock(s.durationS)],
    ['Frames sent', `${n(s.sent)} of ${n(s.frames)}`],
    ['Dropped / resets / key frames', `${n(s.dropped)} / ${n(s.resets)} / ${n(s.idrRequests)}`],
    ['Capture → bridge write', ms(s.bridgeP50, s.bridgeP99)],
    ['Host capture + encode', ms(s.hostP50, s.hostP99)],
    ['Audio lost / errors', `${n(s.audioLost)} / ${n(s.audioErrors)}`],
    ['Input received / sent / dropped', `${n(s.inputReceived)} / ${n(s.inputSent)} / ${n(s.inputDropped)}`],
  ];
}
