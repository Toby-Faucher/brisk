// Typed client for brisk-hostd's /api (same origin). Shapes match hostd's camelCase JSON.
export type Game = {
  appid: number;
  name: string;
  installed: boolean;
  sizeBytes: number | null;
  lastPlayed: number | null /* unix s */;
  playtimeMin: number;
  cover: string;
  /** Steam is running it on the host now: Play streams it without relaunching. */
  running: boolean;
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
};
/** One clipboard entry: text, or a PNG image. */
export type Clip = { kind: 'text'; text: string } | { kind: 'image'; blob: Blob };
/** Where hostd saved an uploaded file. */
export type Saved = { path: string; size: number };
export type SessionRec = { appid: number | null; name: string; started: number /* unix ms */; ended: number | null; host: string; preset: Preset };

export class ApiError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
  }
}

const SECRET_KEY = 'brisk.secret';

/** hostd prints `/?s=<secret>`: keep it for this tab and take it out of the address bar. */
export function captureSecret() {
  try {
    const u = new URL(location.href);
    const s = u.searchParams.get('s');
    if (!s) return;
    sessionStorage.setItem(SECRET_KEY, s);
    u.searchParams.delete('s');
    history.replaceState(history.state, '', u.href);
  } catch {}
}
captureSecret();

function secret(): string | null {
  try {
    return sessionStorage.getItem(SECRET_KEY);
  } catch {
    return null;
  }
}

function auth(headers: Record<string, string> = {}): Record<string, string> {
  const s = secret();
  if (s) headers['x-brisk-secret'] = s;
  return headers;
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
  if (!r.ok) throw apiError(r.status, r.statusText, await r.text().catch(() => ''));
  return r;
}

async function call<T>(method: 'GET' | 'POST' | 'DELETE', path: string, body?: unknown): Promise<T> {
  const headers = auth(body === undefined ? {} : { 'content-type': 'application/json' });
  const r = await send(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
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
function uploadFile(f: File, progress: (loaded: number) => void): Promise<Saved> {
  return new Promise((ok, no) => {
    const x = new XMLHttpRequest();
    x.open('PUT', `/api/files/${encodeURIComponent(f.name)}`);
    for (const [k, v] of Object.entries(auth())) x.setRequestHeader(k, v);
    x.upload.onprogress = (e) => progress(e.loaded);
    x.onload = () => (x.status === 200 ? ok(JSON.parse(x.responseText)) : no(apiError(x.status, x.statusText, x.responseText)));
    x.onerror = () => no(new ApiError('the upload was cut off', 0));
    x.send(f);
  });
}

export const api = {
  host: () => call<HostInfo>('GET', '/api/host'),
  games: () => call<{ games: Game[]; ownedAvailable: boolean }>('GET', '/api/games'),
  stream: () => call<StreamInfo>('GET', '/api/stream'),
  startStream: (req: StreamReq) => call<StreamInfo>('POST', '/api/stream', req),
  stopStream: () => call<StreamInfo>('DELETE', '/api/stream'),
  launch: (appid: number) => call<void>('POST', '/api/launch', { appid }),
  install: (appid: number) => call<void>('POST', '/api/install', { appid }),
  restartSunshine: () => call<void>('POST', '/api/sunshine/restart'),
  sessions: () => call<{ sessions: SessionRec[] }>('GET', '/api/sessions'),
  getClipboard,
  postClipboard,
  uploadFile,
};
