import { get, writable } from 'svelte/store';
import { api, type Codec, type Preset, type StreamInfo, type StreamReq } from './api';
import { settings } from './settings';
import { hevcSupported } from '../lib/video';
import { readyFirst } from '../modules/machines/machines';

/** What hostd keeps per game (camelCase JSON). The custom numbers exist only for `custom`. */
export type GamePrefs = { preset: Preset; codec: Codec; width?: number; height?: number; fps?: number; bitrateKbps?: number; updated: number };
export type PrefsMap = Record<string, GamePrefs>;
/** PUT body: the prefs without `updated`. */
export type PrefsBody = Omit<GamePrefs, 'updated'>;

/** Shared by Library, Home, the Stream page; filled by watchPrefs. */
export const prefs = writable<PrefsMap>({});

/** The saved Home card order (ids); undefined until hostd has answered once. */
export const homeOrder = writable<string[] | undefined>(undefined);

let fetchedAt = -Infinity;
let inflight: Promise<void> | null = null;

/** One GET at a time: Home and its Resume card both ask on mount. */
export function refreshPrefs(): Promise<void> {
  return (inflight ??= api
    .prefs()
    .catch(() => null)
    .then((r) => {
      if (r) {
        fetchedAt = Date.now(); // a failure is retried on the next focus
        prefs.set(r.prefs);
        homeOrder.set(r.homeOrder ?? []);
      }
    })
    .finally(() => (inflight = null)));
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
export async function startGame(
  appid: number | undefined,
  notify: (text: string) => void,
  d: { startStream: (r: StreamReq) => Promise<StreamInfo>; defaultPreset: () => Preset; hevcOk: () => Promise<boolean> } = { startStream: api.startStream, defaultPreset: () => get(settings).defaultPreset, hevcOk: () => hevcSupported() },
) {
  await readyFirst(); // hub mode: wakes the selected machine first (the overlay shows it)
  const req = prefsFor(appid, get(prefs), d.defaultPreset());
  // Only this start falls back; the saved entry stays for a browser that can decode it.
  if (req.codec === 'hevc' && !(await d.hevcOk())) {
    req.codec = 'h264';
    notify('This browser cannot decode HEVC, starting in H.264');
  }
  return d.startStream(req);
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
  const inflight = new Map<number, Promise<void>>();
  const forgotten = new Set<Promise<void>>(); // PUTs whose game was forgotten meanwhile: their answer is dropped
  const flush = () => {
    clearTimeout(timer);
    const p = pending;
    pending = null;
    if (!p) return;
    // quiet on failure: the next change tries again
    const job: Promise<void> = d.put(p.appid, p.body).then(
      (r) => {
        if (!forgotten.delete(job)) d.onSaved(p.appid, r);
      },
      () => void forgotten.delete(job),
    ).finally(() => inflight.get(p.appid) === job && inflight.delete(p.appid));
    inflight.set(p.appid, job);
  };
  return {
    save(appid: number, body: PrefsBody) {
      if (pending && pending.appid !== appid) flush();
      pending = { appid, body };
      clearTimeout(timer);
      timer = setTimeout(flush, d.delay ?? SAVE_DELAY_MS);
    },
    flush,
    /** `appid` was forgotten: drop its pending save and ignore the answer of one in flight. */
    cancel(appid: number) {
      const j = inflight.get(appid);
      if (j) forgotten.add(j);
      if (pending?.appid !== appid) return;
      clearTimeout(timer);
      pending = null;
    },
    /** Resolves once the save of `appid` that is in flight (if any) is over. */
    settled: (appid: number) => inflight.get(appid) ?? Promise.resolve(),
  };
}

/** The saver the Stream page uses: writes through to the store once hostd has it. */
export const saver = prefSaver({ put: api.putPrefs, onSaved: (id, p) => prefs.update((m) => ({ ...m, [id]: p })) });

export async function forgetPrefs(appid: number, d: { del: (id: number) => Promise<void>; cancel: (id: number) => void; settled: (id: number) => Promise<void> } = { del: api.deletePrefs, cancel: saver.cancel, settled: saver.settled }) {
  d.cancel(appid);
  await d.settled(appid); // a PUT still on its way must land before the DELETE, or it would bring the entry back
  await d.del(appid);
  prefs.update((m) => {
    const { [appid]: _, ...rest } = m;
    return rest;
  });
}
