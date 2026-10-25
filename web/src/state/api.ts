// Typed client for brisk-hostd's /api (same origin). Shapes match hostd's camelCase JSON.
export type Game = {
  appid: number;
  name: string;
  installed: boolean;
  sizeBytes: number | null;
  lastPlayed: number | null /* unix s */;
  playtimeMin: number;
  cover: string;
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
export type StreamReq = { preset: Preset; width?: number; height?: number; fps?: number; bitrateKbps?: number; appid?: number };
export type StreamInfo = {
  state: 'idle' | 'starting' | 'live';
  preset: Preset | null;
  appid: number | null;
  wt: string | null;
  hash: string | null;
};
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

async function call<T>(method: 'GET' | 'POST' | 'DELETE', path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (method !== 'GET') {
    const s = secret();
    if (s) headers['x-brisk-secret'] = s;
  }
  if (body !== undefined) headers['content-type'] = 'application/json';
  const r = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  if (!r.ok) {
    let msg = `${r.status} ${r.statusText}`.trim();
    try {
      const j = await r.json();
      if (typeof j?.error === 'string') msg = j.error;
    } catch {}
    throw new ApiError(msg, r.status);
  }
  return (r.status === 204 ? undefined : await r.json()) as T;
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
};
