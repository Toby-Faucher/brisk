// Small formatters shared by Home, Library, Machines and History.

/** Shown in place of a value that is not known. */
export const DASH = '–';

export const fmtTemp = (c: number | null) => (c == null ? DASH : `${Math.round(c)} °C`);

export function fmtUptime(s: number | null): string {
  if (s == null) return DASH;
  const h = Math.floor(s / 3600);
  if (h < 1) return `${Math.floor(s / 60)} min`;
  return h < 24 ? `${h} h` : `${Math.floor(h / 24)} d ${h % 24} h`;
}

/** `ms` and `now` are unix milliseconds. */
export function fmtAgo(ms: number | null, now = Date.now()): string {
  if (ms == null) return DASH;
  const m = Math.floor((now - ms) / 60_000);
  if (m < 1) return 'just now';
  if (m < 60) return `${m} min ago`;
  return m < 1440 ? `${Math.floor(m / 60)} h ago` : `${Math.floor(m / 1440)} d ago`;
}

export function fmtDuration(start: number, end: number | null): string {
  if (end == null) return 'live';
  const m = Math.round((end - start) / 60_000);
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, '0')}`;
}

/** A unix-seconds time as a short date in the browser's locale, like "3 Oct 2026". */
export const fmtDate = (s: number) => new Date(s * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
