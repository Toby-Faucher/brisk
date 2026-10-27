// Typed client for brisk-hostd's /api (same origin). Shapes match hostd's camelCase JSON.
export type Game = {
  appid: number;
  name: string;
  installed: boolean;
  sizeBytes: number | null;
  lastPlayed: number | null /* unix s */;
  playtimeMin: number;
  /** minutes played in the last two weeks */
  playtime2wkMin: number;
  cover: string;
  hero: string;
  logo: string;
  /** Steam is running it on the host now: Play streams it without relaunching. */
  running: boolean;
};
/** Store page facts for the detail panel (GET /api/games/{appid}/info). `available` false: the Store had nothing. */
export type GameInfo = {
  available: boolean;
  /** plain text, never HTML */
  description: string;
  genres: string[];
  controller: 'full' | 'partial' | null;
  singleplayer: boolean;
  coop: boolean;
  remotePlayTogether: boolean;
  release: string | null;
  metacritic: number | null;
  /** thumbnail URLs, at most 6 */
  screenshots: string[];
};
export type HostInfo = {
  name: string;
  sunshine: 'ready' | 'busy' | 'down';
  uptimeS: number | null;
  cpuTempC: number | null;
  gpuTempC: number | null;
  path: 'direct' | 'relay' | null;
  relay: string | null;
};
export type Preset = 'fast' | 'balanced' | 'crisp' | 'custom';
export type Codec = 'h264' | 'hevc';
/** `codec` defaults to h264 on hostd. */
export type StreamReq = { preset: Preset; width?: number; height?: number; fps?: number; bitrateKbps?: number; codec?: Codec; appid?: number };
export type StreamInfo = {
  state: 'idle' | 'starting' | 'live';
  preset: Preset | null;
  codec: Codec | null;
  appid: number | null;
  wt: string | null;
  hash: string | null;
  /** The live session's id (its record's `started`), where the page PUTs its series. */
  started?: number | null;
  /** Seconds since `started` on hostd's monotonic clock, as of this answer: the series time base. */
  elapsedS?: number | null;
};
/** One clipboard entry: text, or a PNG image. */
export type Clip = { kind: 'text'; text: string } | { kind: 'image'; blob: Blob };
/** Where hostd saved an uploaded file. */
export type Saved = { path: string; size: number };
/** The bridge's end-of-session report (docs/m0-baseline.md); null fields were missing from it. */
export type SessionStats = {
  durationS: number | null;
  frames: number | null;
  sent: number | null;
  dropped: number | null;
  resets: number | null;
  idrRequests: number | null;
  bridgeP50: number | null;
  bridgeP99: number | null;
  hostP50: number | null;
  hostP99: number | null;
  audioLost: number | null;
  audioErrors: number | null;
  inputReceived: number | null;
  inputSent: number | null;
  inputDropped: number | null;
  gate: 'pass' | 'fail' | null;
};
/** codec, bitrate and stats are null (or absent) on records from before they were kept. */
export type SessionRec = {
  appid: number | null;
  name: string;
  started: number /* unix ms */;
  ended: number | null;
  host: string;
  preset: Preset;
  codec?: Codec | null;
  bitrateKbps?: number | null;
  stats?: SessionStats | null;
};
/** `[t s since started, delay ms, fps, Mbit/s]`, one per 5 s. */
export type SeriesPoint = [number, number, number, number];

import { writable } from 'svelte/store';
import type { GamePrefs, PrefsBody, PrefsMap } from './prefs';

export class ApiError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
  }
}

const SECRET_KEY = 'brisk.secret';

/** hostd answered 403: no valid secret and no allowed tailnet login. The Shell shows how to get in. */
export const denied = writable(false);

/** hostd prints `/?s=<secret>`: keep it (this tab, and localStorage so a bookmark of the bare URL
 *  keeps working; the secret file on bawn persists across restarts) and take it out of the address bar. */
export function captureSecret() {
  try {
    const u = new URL(location.href);
    const s = u.searchParams.get('s');
    if (!s) return;
    sessionStorage.setItem(SECRET_KEY, s);
    try {
      localStorage.setItem(SECRET_KEY, s);
    } catch {}
    u.searchParams.delete('s');
    history.replaceState(history.state, '', u.href);
  } catch {}
}
captureSecret();

function stored(st: () => Storage): string | null {
  try {
    return st().getItem(SECRET_KEY);
  } catch {
    return null;
  }
}

/** None is fine: through `tailscale serve` hostd may know the login instead. */
function secret(): string | null {
  return stored(() => sessionStorage) ?? stored(() => localStorage);
}

/** `x-brisk-csrf` on every call: hostd only trusts a tailnet login on requests carrying it (a
 *  custom header cannot be sent cross-site without a CORS preflight, which hostd never grants). */
function auth(headers: Record<string, string> = {}): Record<string, string> {
  const s = secret();
  if (s) headers['x-brisk-secret'] = s;
  headers['x-brisk-csrf'] = '1';
  return headers;
}

function seen(status: number) {
  denied.set(status === 403);
}

/** hostd's `{error}` text, else the status line. */
function apiError(status: number, statusText: string, body: string): ApiError {
  let msg = `${status} ${statusText}`.trim();
  try {
    const j = JSON.parse(body);
    if (typeof j?.error === 'string') msg = j.error;
  } catch {}
  return new ApiError(msg, status);
}

async function send(path: string, init: RequestInit): Promise<Response> {
  const r = await fetch(path, init);
  seen(r.status);
  if (!r.ok) throw apiError(r.status, r.statusText, await r.text().catch(() => ''));
  return r;
}

async function call<T>(method: 'GET' | 'POST' | 'PUT' | 'DELETE', path: string, body?: unknown, keepalive = false): Promise<T> {
  const headers = auth(body === undefined ? {} : { 'content-type': 'application/json' });
  const r = await send(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), keepalive });
  return (r.status === 204 ? undefined : await r.json()) as T;
}

/** `since` null: only the seq. Text comes as JSON, an image as the PNG body. */
async function getClipboard(since: number | null): Promise<{ seq: number; clip: Clip | null }> {
  const r = await send(`/api/clipboard${since === null ? '' : `?since=${since}`}`, { headers: auth() });
  const seq = Number(r.headers.get('x-brisk-seq'));
  if (r.status === 204) return { seq, clip: null };
  if (r.headers.get('content-type')?.startsWith('image/png')) return { seq, clip: { kind: 'image', blob: await r.blob() } };
  return { seq, clip: { kind: 'text', text: (await r.json()).text } };
}

async function postClipboard(c: Clip): Promise<void> {
  const ct = c.kind === 'text' ? 'text/plain;charset=utf-8' : 'image/png';
  await send('/api/clipboard', { method: 'POST', headers: auth({ 'content-type': ct }), body: c.kind === 'text' ? c.text : c.blob });
}

/** XHR, not fetch: fetch has no upload progress. hostd streams the body to ~/Downloads/brisk. */
function uploadFile(f: File, progress: (loaded: number) => void, signal?: AbortSignal): Promise<Saved> {
  return new Promise((ok, no) => {
    const x = new XMLHttpRequest();
    signal?.addEventListener('abort', () => x.abort());
    x.onabort = () => no(new ApiError('cancelled', 0));
    x.open('PUT', `/api/files/${encodeURIComponent(f.name)}`);
    for (const [k, v] of Object.entries(auth())) x.setRequestHeader(k, v);
    x.upload.onprogress = (e) => progress(e.loaded);
    x.onload = () => (seen(x.status), x.status === 200 ? ok(JSON.parse(x.responseText)) : no(apiError(x.status, x.statusText, x.responseText)));
    x.onerror = () => no(new ApiError('the upload was cut off', 0));
    x.send(f);
  });
}

export const api = {
  host: () => call<HostInfo>('GET', '/api/host'),
  games: () => call<{ games: Game[]; ownedAvailable: boolean }>('GET', '/api/games'),
  gameInfo: (appid: number) => call<GameInfo>('GET', `/api/games/${appid}/info`),
  stream: () => call<StreamInfo>('GET', '/api/stream'),
  startStream: (req: StreamReq) => call<StreamInfo>('POST', '/api/stream', req),
  stopStream: () => call<StreamInfo>('DELETE', '/api/stream'),
  launch: (appid: number) => call<void>('POST', '/api/launch', { appid }),
  install: (appid: number) => call<void>('POST', '/api/install', { appid }),
  /** Closes a running game (hostd escalates; `force` allows the last resort). Takes up to ~25 s. */
  quit: (appid: number, force = false) => call<{ state: 'closed' | 'still-running' }>('POST', '/api/quit', { appid, force }),
  restartSunshine: () => call<void>('POST', '/api/sunshine/restart'),
  prefs: () => call<{ prefs: PrefsMap; homeOrder?: string[] }>('GET', '/api/prefs'),
  /** Home card order (hostd cleans it and answers with what it kept); reset forgets it. */
  putLayout: (homeOrder: string[]) => call<{ homeOrder: string[] }>('PUT', '/api/prefs/layout', { homeOrder }),
  resetLayout: () => call<void>('DELETE', '/api/prefs/layout'),
  putPrefs: (appid: number, b: PrefsBody) => call<GamePrefs>('PUT', `/api/prefs/${appid}`, b, true), // keepalive: a flush on pagehide can finish after the page is gone
  deletePrefs: (appid: number) => call<void>('DELETE', `/api/prefs/${appid}`),
  /** Each row carries `spark`: up to 24 mean-binned delays of its series (empty without one). */
  sessions: () => call<{ sessions: (SessionRec & { spark?: number[] })[] }>('GET', '/api/sessions'),
  session: (started: number) => call<{ session: SessionRec; series: SeriesPoint[] }>('GET', `/api/sessions/${started}`),
  putSeries: (started: number, points: SeriesPoint[]) => call<void>('PUT', `/api/sessions/${started}/series`, { points }, true), // keepalive: sent on pagehide
  getClipboard,
  postClipboard,
  uploadFile,
};
