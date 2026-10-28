// Typed client for brisk-hostd's /api (same origin). Shapes match hostd's camelCase JSON.
/** What a game's source can do with it (hostd sends these; for an old hostd see gameCaps in gameId.ts). */
export type Caps = {
  install: boolean;
  controls: boolean;
  achievements: boolean;
  launchOptions: boolean;
  saves: boolean;
  screenshots: boolean;
};
export type Game = {
  /** The key to use: `steam:620`, `emu:dolphin:mario-kart-wii`. An old hostd sends none (see gameKey in gameId.ts). */
  id?: string;
  source?: 'steam' | 'emu';
  caps?: Caps;
  /** The old number as text (for a release). */
  legacyId?: string;
  /** `wii`, `gamecube`; only when known. */
  system?: string;
  /** The Steam appid; for an emulator game the old hash. Not a key: use gameKey. */
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
  /** Achievement counts once the host has fetched them (it warms its cache in the background); null until then. */
  achUnlocked: number | null;
  achTotal: number | null;
  /** Installed, and the host saw a patch newer than your last play (from its news cache). */
  updated: boolean;
  /** Steam's own update state for an installed game (from its manifest); distinct from `updated`. */
  update: 'needed' | 'running' | null;
  /** From the host's store-info cache (null until it has fetched this game, or when the Store has none). */
  controller: 'full' | 'partial' | null;
  /** Co-op or Remote Play Together, same source. */
  coop: boolean;
  /** Emulator games only (e.g. 'dolphin'): these have no art, so the cover is a title card with this badge. */
  emulator?: string;
};
/** One achievement; `description` is empty for hidden ones, `icon` is an https URL or empty. Text only: never HTML. */
export type Ach = { id: string; name: string; description: string; icon: string; unlockedAt: number | null };
/** GET /api/games/{appid}/achievements. `available` false (no stats, private profile): total is 0. */
export type Achievements = { available: boolean; unlocked: number; total: number; list: Ach[] };
/** One headline; `title` is text only, `url` is https or empty. `patch`: the game's own announcements feed. */
export type NewsItem = { title: string; url: string; date: number; patch: boolean };
/** GET /api/games/{appid}/news; an empty list when Steam had nothing. */
export type News = { items: NewsItem[] };
/** One friend who is online or in a game. `name` is text only; `avatar` is an https Steam URL or empty. */
export type Friend = {
  name: string;
  avatar: string;
  state: 'online' | 'away' | 'busy' | 'snooze';
  playing: string | null;
  appid: number | null;
};
/** GET /api/friends. `available` false: the list is private or Steam did not answer. */
export type Friends = { available: boolean; online: Friend[] };
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
  /** the full-size image for each thumbnail, same order (absent in a cache entry written before it existed) */
  screenshotsFull?: string[];
};
/** GET /api/games/{appid}/local: screenshot file names on the host (newest first, at most 60) and a cloud-save summary. */
export type GameLocal = { screenshots: string[]; saves: { files: number; bytes: number; modified: number /* unix s */ } | null };
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
export type StreamReq = {
  preset: Preset;
  width?: number;
  height?: number;
  fps?: number;
  bitrateKbps?: number;
  codec?: Codec;
  /** The game's key (see gameKey); the client turns it into `id` or `appid` for the machine. Omit for the Desktop. */
  game?: string;
};
export type StreamInfo = {
  state: 'idle' | 'starting' | 'live';
  preset: Preset | null;
  codec: Codec | null;
  appid: number | null;
  /** Set by a new hostd. */
  gameId?: string | null;
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
  /** Set by a new hostd (older rows get it at its first start). */
  gameId?: string | null;
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

/** An emulator controller mapping (GET/PUT/reset /api/emulators/dolphin/controls/{type}); see docs/plans/2026-10-28-emulator-controls-v2.md. */
export type ControllerType = 'classic' | 'gamecube';
/** Game control ids of the type (`a`, `zl`, `ms`...); the key sets are in modules/settings/emulators/mapping.ts. */
export type Bindings = Record<string, string | null>;
export type ControlsData = {
  emulator: string;
  type: ControllerType;
  controller: string;
  source: 'file' | 'default';
  rev: string;
  running: boolean;
  appliesOn: 'next-launch';
  /** GameCube only: Dolphin's port 1 is set to a standard controller. */
  portReady?: boolean;
  bindings: Bindings;
  deadZone: Record<string, number>;
  custom: Record<string, boolean>;
};
/** PUT body; `rev` left out is last write wins. */
export type ControlsPut = { rev?: string; bindings: Bindings; deadZone: Record<string, number> };

import { writable } from 'svelte/store';
import type { GamePrefs, PrefsBody, PrefsMap } from './prefs';
import type { GameNote, Notes, Tag } from './notes';
import type { Machine } from './machines';
import { gameSegment, toIdForm, toNumberForm } from './gameId';

/** Stage 2 uploads (docs/plans/2026-10-28-stage2-upload.md). */
export type UploadStart = { id: string; chunk: number; offset: number };
export type UploadStatus = { id: string; name: string; offset: number; size: number; state: 'open' | 'busy' };
export type UploadDone = { system: string; registered: boolean; duplicate: boolean; game: Game | null };
/** What one machine's upload calls look like to the upload engine (state/uploads.ts). */
export interface UploadApi {
  create(name: string, size: number): Promise<UploadStart>;
  put(id: string, offset: number, blob: Blob, signal?: AbortSignal): Promise<{ offset: number }>;
  status(id: string): Promise<UploadStatus>;
  finish(id: string): Promise<UploadDone>;
  abort(id: string): Promise<void>;
}
/** POST /api/library/scan. `removed`: games you removed that the scan left out. */
export type ScanResult = {
  scanned: number;
  added: Game[];
  aliased: number;
  duplicates: number;
  unplayable: number;
  removed: number;
  unknown: number;
  broken: number;
  truncated: boolean;
};

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    /** The parsed JSON body of an error answer, when it had one (a 409 on controls carries `current`). */
    readonly body?: unknown,
  ) {
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

let prefix: string | null = null;
let openGate: (() => void) | null = null;
let gate: Promise<void> = Promise.resolve();
/** machines.ts sets this to the selected machine's id while a hub is detected (null: single-host, paths untouched).
 *  `known` false (hub not probed yet, or no machine chosen yet) holds machine-scoped calls until it is true, so they
 *  never reach the hub's own hostd, which has no Steam. Open by default, so a page without machines.ts is a single host. */
export function setRoute(machineId: string | null, known = true) {
  prefix = machineId;
  if (known) openGate?.(), (openGate = null);
  else if (!openGate) gate = new Promise((r) => (openGate = r));
}

/** Machine-scoped paths go through the hub's `/api/m/{id}/`; prefs, notes and machines stay on the hub itself. */
const SCOPED =
  /^\/api\/(host|games|friends|stream|launch|install|quit|sessions|sunshine|clipboard|files|emulators|uploads|library)(?=[/?]|$)/;
export function route(path: string): string {
  return prefix && SCOPED.test(path) ? `/api/m/${encodeURIComponent(prefix)}/${path.slice(5)}` : path;
}

// Which machines (and the hub's own hostd, for notes and prefs) speak string ids. The decision lives here: callers pass a
// game key and these turn it into the form on the wire. '' is a single host. An unknown machine gets numbers, which
// every hostd accepts for a Steam game.
const idSchemes = new Map<string, boolean>();
let hubSeen = false;
const noteScheme = (machine: string | null, r: { games: Game[]; idScheme?: number }) =>
  idSchemes.set(machine ?? '', r.idScheme === 1 || (r.games ?? []).some((g) => g.id !== undefined));
const machineIds = () => idSchemes.get(prefix ?? '') === true;
/** The hub's own hostd keeps notes and prefs. It speaks ids when it said so (a key with ':'), or it is the one host. */
const hubIds = () => hubSeen || (prefix === null && idSchemes.get('') === true);
const seeIds = (m: Record<string, unknown>) => void (Object.keys(m).some((k) => k.includes(':')) && (hubSeen = true));
const machineForm = (key: string) => (machineIds() ? toIdForm(key) : toNumberForm(key));
const hubForm = (key: string) => (hubIds() ? toIdForm(key) : toNumberForm(key));
/** The body that names a game: `{id}` for a new hostd, `{appid}` for an old one. */
function nameGame(key: string): { id: string } | { appid: number } {
  const k = machineForm(key);
  return /^[1-9]\d*$/.test(k) && !machineIds() ? { appid: Number(k) } : { id: k };
}
const gamePath = (key: string, tail: string) => `/api/games/${gameSegment(machineForm(key))}${tail}`;

function seen(status: number) {
  denied.set(status === 403);
}

/** hostd's `{error}` text, else the status line. */
function apiError(status: number, statusText: string, body: string): ApiError {
  let msg = `${status} ${statusText}`.trim();
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
    const e = (parsed as { error?: unknown } | null)?.error;
    if (typeof e === 'string') msg = e;
  } catch {}
  return new ApiError(msg, status, parsed);
}

async function send(path: string, init: RequestInit): Promise<Response> {
  if (openGate && SCOPED.test(path)) await gate; // only while closed: an open gate adds no tick
  const r = await fetch(route(path), init);
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

/** An object URL for one host screenshot: fetched with the auth headers, which an `<img src>` cannot send. Revoke it when done. */
async function screenshot(key: string, name: string): Promise<string> {
  const r = await send(gamePath(key, `/screenshots/${encodeURIComponent(name)}`), { headers: auth() });
  return URL.createObjectURL(await r.blob());
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
    x.open('PUT', route(`/api/files/${encodeURIComponent(f.name)}`));
    for (const [k, v] of Object.entries(auth())) x.setRequestHeader(k, v);
    x.upload.onprogress = (e) => progress(e.loaded);
    x.onload = () => (
      seen(x.status), x.status === 200 ? ok(JSON.parse(x.responseText)) : no(apiError(x.status, x.statusText, x.responseText))
    );
    x.onerror = () => no(new ApiError('the upload was cut off', 0));
    x.send(f);
  });
}

/** Upload calls for one machine: `machineId` names it through the hub, null follows the selected machine (or a single host). */
function uploadsFor(machineId: string | null): UploadApi {
  const base = machineId ? `/api/m/${encodeURIComponent(machineId)}` : '/api';
  const u = (id: string) => `${base}/uploads/${encodeURIComponent(id)}`;
  return {
    create: (name, size) => call<UploadStart>('POST', `${base}/uploads`, { name, size }),
    async put(id, offset, blob, signal) {
      const r = await send(u(id), { method: 'PUT', headers: auth({ 'x-offset': String(offset) }), body: blob, signal });
      return (await r.json()) as { offset: number };
    },
    status: (id) => call<UploadStatus>('GET', u(id)),
    finish: (id) => call<UploadDone>('POST', `${u(id)}/finish`),
    abort: (id) => call<void>('DELETE', u(id)),
  };
}

/** Scan the games folder of one machine (null: the selected one). `includeRemoved` brings back games you removed. */
function scanLibrary(machineId: string | null, includeRemoved = false): Promise<ScanResult> {
  const base = machineId ? `/api/m/${encodeURIComponent(machineId)}` : '/api';
  return call<ScanResult>('POST', `${base}/library/scan${includeRemoved ? '?include_removed=1' : ''}`);
}

const q = (refresh?: boolean) => (refresh ? '?refresh=1' : '');

/** Calls for one machine through the hub, whichever machine is selected. */
function forMachine(id: string) {
  const base = `/api/m/${encodeURIComponent(id)}`; // not in SCOPED, so route() leaves it alone
  return {
    games: (refresh?: boolean) => call<GamesAnswer>('GET', `${base}/games${q(refresh)}`).then((r) => (noteScheme(id, r), r)),
    sessions: () => call<{ sessions: (SessionRec & { spark?: number[] })[] }>('GET', `${base}/sessions`),
    host: () => call<HostInfo>('GET', `${base}/host`),
  };
}

type GamesAnswer = { games: Game[]; ownedAvailable: boolean; idScheme?: number };

export const api = {
  forMachine,
  /** Hub only, never routed. */
  machines: () => call<{ machines: Machine[] }>('GET', '/api/machines'),
  power: (id: string, action: 'start' | 'shutdown') =>
    call<{ state: string }>('POST', `/api/machines/${encodeURIComponent(id)}/power`, { action }),
  host: () => call<HostInfo>('GET', '/api/host'),
  /** `refresh` skips hostd's Steam caches (`?refresh=1`). */
  games: (refresh?: boolean) => call<GamesAnswer>('GET', `/api/games${q(refresh)}`).then((r) => (noteScheme(prefix, r), r)),
  uploadsFor,
  scanLibrary,
  /** Forgets an emulator game's library record. The file stays on disk. Sends the id form this machine speaks. */
  removeGame: (key: string) => call<void>('DELETE', gamePath(key, '')),
  gameInfo: (key: string, refresh?: boolean) => call<GameInfo>('GET', gamePath(key, `/info${q(refresh)}`)),
  gameAchievements: (key: string, refresh?: boolean) => call<Achievements>('GET', gamePath(key, `/achievements${q(refresh)}`)),
  gameNews: (key: string, refresh?: boolean) => call<News>('GET', gamePath(key, `/news${q(refresh)}`)),
  emulatorControls: (type: ControllerType) => call<ControlsData>('GET', `/api/emulators/dolphin/controls/${type}`),
  putEmulatorControls: (type: ControllerType, b: ControlsPut) => call<ControlsData>('PUT', `/api/emulators/dolphin/controls/${type}`, b),
  resetEmulatorControls: (type: ControllerType) => call<ControlsData>('POST', `/api/emulators/dolphin/controls/${type}/reset`),
  gameLocal: (key: string) => call<GameLocal>('GET', gamePath(key, '/local')),
  screenshot,
  friends: () => call<Friends>('GET', '/api/friends'),
  stream: () => call<StreamInfo>('GET', '/api/stream'),
  startStream: (req: StreamReq) => {
    const { game, ...rest } = req;
    return call<StreamInfo>('POST', '/api/stream', game === undefined ? rest : { ...rest, ...nameGame(game) });
  },
  stopStream: () => call<StreamInfo>('DELETE', '/api/stream'),
  launch: (key: string) => call<void>('POST', '/api/launch', nameGame(key)),
  install: (key: string) => call<void>('POST', '/api/install', nameGame(key)),
  /** Closes a running game (hostd escalates; `force` allows the last resort). Takes up to ~25 s. */
  quit: (key: string, force = false) => call<{ state: 'closed' | 'still-running' }>('POST', '/api/quit', { ...nameGame(key), force }),
  restartSunshine: () => call<void>('POST', '/api/sunshine/restart'),
  prefs: () =>
    call<{ prefs: PrefsMap; homeOrder?: string[]; machinesOrder?: string[]; machineColors?: Record<string, string> }>(
      'GET',
      '/api/prefs',
    ).then((r) => (seeIds(r.prefs ?? {}), r)),
  /** Card orders and machine colours (hostd cleans them and answers with what it kept); a bare array is the Home order. An omitted key keeps its value; a colour of null removes that machine's. Reset forgets all. */
  putLayout: (l: string[] | { homeOrder?: string[]; machinesOrder?: string[]; machineColors?: Record<string, string | null> }) =>
    call<{ homeOrder: string[]; machinesOrder?: string[]; machineColors?: Record<string, string> }>(
      'PUT',
      '/api/prefs/layout',
      Array.isArray(l) ? { homeOrder: l } : l,
    ),
  resetLayout: () => call<void>('DELETE', '/api/prefs/layout'),
  putPrefs: (key: string, b: PrefsBody) => call<GamePrefs>('PUT', `/api/prefs/${gameSegment(hubForm(key))}`, b, true), // keepalive: a flush on pagehide can finish after the page is gone
  deletePrefs: (key: string) => call<void>('DELETE', `/api/prefs/${gameSegment(hubForm(key))}`),
  notes: () => call<Notes>('GET', '/api/notes').then((r) => (seeIds(r.games ?? {}), r)),
  /** Hub only: move notes and prefs kept under an old emulator number to its new id. Safe to repeat; an old hostd has no such route. */
  remapIds: (map: Record<string, string>) => call<{ notes: number; prefs: number }>('POST', '/api/ids/remap', { map }),
  /** Create or replace a tag; hostd answers with what it stored (name trimmed). */
  putTag: (id: string, tag: Tag) => call<Tag>('PUT', `/api/notes/tags/${id}`, tag),
  /** Also takes the tag off every game. */
  deleteTag: (id: string) => call<void>('DELETE', `/api/notes/tags/${id}`),
  /** hostd drops unknown tag ids and answers with what it stored. */
  putGameNote: (key: string, b: GameNote) => call<GameNote>('PUT', `/api/notes/games/${gameSegment(hubForm(key))}`, b, true), // keepalive: flushed on pagehide
  /** Each row carries `spark`: up to 24 mean-binned delays of its series (empty without one). */
  sessions: () => call<{ sessions: (SessionRec & { spark?: number[] })[] }>('GET', '/api/sessions'),
  session: (started: number) => call<{ session: SessionRec; series: SeriesPoint[] }>('GET', `/api/sessions/${started}`),
  putSeries: (started: number, points: SeriesPoint[]) => call<void>('PUT', `/api/sessions/${started}/series`, { points }, true), // keepalive: sent on pagehide
  getClipboard,
  postClipboard,
  uploadFile,
};
