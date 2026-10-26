import { get, writable } from 'svelte/store';
import { api, type Codec, type Preset, type StreamInfo, type StreamReq } from './api';
import { settings } from './settings';

/** What hostd keeps per game (camelCase JSON). The custom numbers exist only for `custom`. */
export type GamePrefs = { preset: Preset; codec: Codec; width?: number; height?: number; fps?: number; bitrateKbps?: number; updated: number };
export type PrefsMap = Record<string, GamePrefs>;
/** PUT body: the prefs without `updated`. */
export type PrefsBody = Omit<GamePrefs, 'updated'>;

/** Shared by Library, Home, the Stream page; filled by watchPrefs. */
export const prefs = writable<PrefsMap>({});

let fetchedAt = -Infinity;

export async function refreshPrefs() {
  fetchedAt = Date.now();
  const r = await api.prefs().catch(() => null);
  if (r) prefs.set(r.prefs);
}

/** Fetch if never fetched (or older than a minute), then again on window focus at most once a minute. */
export function watchPrefs(win: EventTarget = window): () => void {
  const onFocus = () => {
    if (Date.now() - fetchedAt >= 60_000) void refreshPrefs();
  };
  onFocus();
  win.addEventListener('focus', onFocus);
  return () => win.removeEventListener('focus', onFocus);
}

/** The one place that decides how a game starts: its saved quality, else the default preset on h264. No appid = the Desktop: defaults, no lookup. */
export function prefsFor(appid: number | undefined, map: PrefsMap, defaultPreset: Preset): StreamReq {
  const p = appid === undefined ? undefined : map[appid];
  if (appid === undefined) return { preset: defaultPreset, codec: 'h264' };
  if (!p) return { preset: defaultPreset, codec: 'h264', appid };
  const req: StreamReq = { preset: p.preset, codec: p.codec, appid };
  if (p.preset === 'custom') for (const k of ['width', 'height', 'fps', 'bitrateKbps'] as const) if (p[k] !== undefined) req[k] = p[k];
  return req;
}

/** Start a stream (Library click, Resume card, Stream Start). Deps are injectable for tests. */
export function startGame(
  appid: number | undefined,
  d: { startStream: (r: StreamReq) => Promise<StreamInfo>; defaultPreset: () => Preset } = { startStream: api.startStream, defaultPreset: () => get(settings).defaultPreset },
) {
  return d.startStream(prefsFor(appid, get(prefs), d.defaultPreset()));
}

/** What to save after a quality change: a chip keeps the live codec, Advanced names its own. */
export function savedBody(req: StreamReq, liveCodec: Codec | null): PrefsBody {
  const b: PrefsBody = { preset: req.preset, codec: req.codec ?? liveCodec ?? 'h264' };
  if (req.preset === 'custom') for (const k of ['width', 'height', 'fps', 'bitrateKbps'] as const) if (req[k] !== undefined) b[k] = req[k];
  return b;
}

const MBPS: Record<Preset, number | undefined> = { fast: 10, balanced: 20, crisp: 40, custom: undefined };
const NAME: Record<Preset, string> = { fast: 'Fast', balanced: 'Balanced', crisp: 'Crisp', custom: 'Custom' };

/** Small label for the Library: 'Crisp', 'HEVC · 40 Mbps', 'Custom · 8 Mbps'. */
export function prefsChip(p: GamePrefs): string {
  const mbps = (p.preset === 'custom' ? (p.bitrateKbps ?? 20000) / 1000 : MBPS[p.preset]) ?? 20;
  if (p.codec === 'hevc') return `HEVC · ${mbps} Mbps`;
  return p.preset === 'custom' ? `Custom · ${mbps} Mbps` : NAME[p.preset];
}

export const SAVE_DELAY_MS = 1000;

/** Debounced, last-wins save for one game at a time; a change for another game saves the pending one first. */
export function prefSaver(d: { put: (appid: number, b: PrefsBody) => Promise<GamePrefs>; onSaved: (appid: number, p: GamePrefs) => void; delay?: number }) {
  let pending: { appid: number; body: PrefsBody } | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const flush = () => {
    clearTimeout(timer);
    const p = pending;
    pending = null;
    if (p) void d.put(p.appid, p.body).then((r) => d.onSaved(p.appid, r), () => {}); // quiet: the next change tries again
  };
  return {
    save(appid: number, body: PrefsBody) {
      if (pending && pending.appid !== appid) flush();
      pending = { appid, body };
      clearTimeout(timer);
      timer = setTimeout(flush, d.delay ?? SAVE_DELAY_MS);
    },
    flush,
    /** Drop a pending save of `appid` (it was just forgotten). */
    cancel(appid: number) {
      if (pending?.appid !== appid) return;
      clearTimeout(timer);
      pending = null;
    },
  };
}

/** The saver the Stream page uses: writes through to the store once hostd has it. */
export const saver = prefSaver({ put: api.putPrefs, onSaved: (id, p) => prefs.update((m) => ({ ...m, [id]: p })) });

export async function forgetPrefs(appid: number, d: { del: (id: number) => Promise<void>; cancel: (id: number) => void } = { del: api.deletePrefs, cancel: saver.cancel }) {
  d.cancel(appid);
  await d.del(appid);
  prefs.update((m) => {
    const { [appid]: _, ...rest } = m;
    return rest;
  });
}
