import { derived, get, writable, type Readable, type Writable } from 'svelte/store';
import { api, setRoute, type HostInfo } from './api';
import { defaultColor, normalizeHex } from '../ui/color';
import { safeGet, safeSet } from './storage';

export type MachineState = 'off' | 'starting' | 'stopping' | 'ready' | 'unreachable';
export type Machine = {
  id: string;
  name: string;
  state: MachineState;
  canPower: boolean;
  sunshine: 'ready' | 'busy' | 'down' | null;
  host: HostInfo | null;
  /** The machine's own base URL, which the browser times directly. */
  url: string;
  /** How long the hub's last /api/host probe took (ms); null when it did not answer. */
  rttMs: number | null;
  /** Proxmox VM load (power machines only); cpuPct is a percentage, 0..100, as sent by the hub; null when the call failed. */
  vm: { cpuPct: number; memUsedBytes: number; memTotalBytes: number } | null;
};
/** One latency sample (ms, null = lost): the browser's own timing and the hub's. */
export type PingSample = { t: number; direct: number | null; hub: number | null };
/** Progress of waking a machine; null when nothing is being woken. */
export type Wake = { id: string; phase: 'starting' | 'waiting' | 'failed'; since: number /* Date.now() */; error?: string } | null;

const KEY = 'brisk.machine';
const SLOW = 5_000;
const FAST = 1_500;
const WAKE_MAX = 5 * 60_000;
const UNREACHABLE_MAX = 30_000;

const hubW = writable<boolean | null>(null);
const machinesW = writable<Machine[]>([]);
const wakeW = writable<Wake>(null);

/** null until probed; true when the hub answers GET /api/machines, false on 404 (a plain single host). */
export const hub: Readable<boolean | null> = { subscribe: hubW.subscribe };
export const machines: Readable<Machine[]> = { subscribe: machinesW.subscribe };
export const wake: Readable<Wake> = { subscribe: wakeW.subscribe };

const sel = writable<string | null>(safeGet(() => localStorage, KEY));
export const selected: Writable<string | null> = {
  set(id) {
    safeSet(() => localStorage, KEY, id);
    sel.set(id);
  },
  update: (fn) => selected.set(fn(get(sel))),
  subscribe: sel.subscribe,
};
export const selectedMachine: Readable<Machine | null> = derived([machinesW, sel], ([$m, $s]) => $m.find((m) => m.id === $s) ?? null);

// Machine-scoped api calls follow the selection, but only once a hub is known.
// Until the hub is probed and a machine is chosen the route is unknown: scoped calls wait (see setRoute).
derived([hubW, sel], ([$h, $s]) => [$h ? $s : null, $h === false || ($h === true && $s !== null)] as const).subscribe(([id, known]) =>
  setRoute(id, known),
);

/** One fetch. Fixes the selection before publishing, so routed calls never see an unknown id. */
async function refresh(): Promise<void> {
  try {
    const { machines: list } = await api.machines();
    if (list.length && !list.some((m) => m.id === get(sel))) selected.set(list[0].id);
    machinesW.set(list);
    hubW.set(true);
    loadOrder();
  } catch (e) {
    if ((e as { status?: number }).status === 404) hubW.set(false);
    else if (get(hubW) === null) throw e; // not probed yet: let the caller retry
  }
}

let users = 0;
let timer: ReturnType<typeof setTimeout> | undefined;

async function tick() {
  await refresh().catch(() => {});
  if (!users || get(hubW) === false) return;
  timer = setTimeout(tick, get(machinesW).some((m) => m.state === 'starting') ? FAST : SLOW);
}

/** Probe for a hub and poll it while one exists. Ref-counted: every caller gets a stop. */
export function startMachines(): () => void {
  if (users++ === 0) void tick();
  let done = false;
  return () => {
    if (done) return;
    done = true;
    if (--users === 0) clearTimeout(timer);
  };
}

export async function powerMachine(id: string, action: 'start' | 'shutdown'): Promise<void> {
  await api.power(id, action);
  await refresh().catch(() => {});
}

const waking = new Map<string, Promise<void>>();
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const stateOf = (id: string) => get(machinesW).find((m) => m.id === id)?.state;

async function wakeUp(id: string): Promise<void> {
  await refresh().catch(() => {});
  if (stateOf(id) === 'ready') return;
  const since = Date.now();
  try {
    wakeW.set({ id, phase: 'starting', since });
    if (stateOf(id) === 'off') await api.power(id, 'start');
    wakeW.set({ id, phase: 'waiting', since });
    let down: number | null = null;
    for (;;) {
      await sleep(FAST);
      await refresh().catch(() => {});
      const s = stateOf(id);
      if (s === 'ready') return void wakeW.set(null);
      if (s === undefined) throw new Error('unknown machine');
      down = s === 'unreachable' ? (down ?? Date.now()) : null;
      if (down !== null && Date.now() - down >= UNREACHABLE_MAX) throw new Error('machine is unreachable');
      if (Date.now() - since >= WAKE_MAX) throw new Error('machine did not start in time');
    }
  } catch (e) {
    wakeW.set({ id, phase: 'failed', since, error: e instanceof Error ? e.message : String(e) });
    throw e;
  }
}

/** Starts the machine when it is off and resolves once it is ready; concurrent calls share one wait. */
export function ensureReady(id: string): Promise<void> {
  let p = waking.get(id);
  if (!p) {
    p = wakeUp(id).finally(() => waking.delete(id));
    waking.set(id, p);
  }
  return p;
}

export async function selectMachine(id: string): Promise<void> {
  selected.set(id);
  if (get(machinesW).find((m) => m.id === id)?.state === 'off') await ensureReady(id);
}

const orderW = writable<string[]>([]);
/** The saved card order (ids); machines not in it follow in server order (see orderedMachines). */
export const machinesOrder: Readable<string[]> = { subscribe: orderW.subscribe };
export const orderedMachines: Readable<Machine[]> = derived([machinesW, orderW], ([$m, $o]) => [
  ...$o.flatMap((id) => $m.find((m) => m.id === id) ?? []),
  ...$m.filter((m) => !$o.includes(m.id)),
]);

const colorsW = writable<Record<string, string>>({});
/** Saved machine colours (`#rrggbb`), from the same prefs fetch as the order. */
export const machineColors: Readable<Record<string, string>> = { subscribe: colorsW.subscribe };
/** Every machine's colour: the saved one, else the palette colour for its position in server order. */
export const machineHue: Readable<Record<string, string>> = derived([machinesW, colorsW], ([$m, $c]) =>
  Object.fromEntries($m.map((m, i) => [m.id, $c[m.id] ?? defaultColor(i)])),
);

/** Normalizes what the server sent; absent or bad entries are dropped. */
function cleanColors(raw: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  if (raw && typeof raw === 'object')
    for (const [id, v] of Object.entries(raw)) {
      const hex = typeof v === 'string' ? normalizeHex(v) : null;
      if (hex) out[id] = hex;
    }
  return out;
}

// Loaded once, on hub detection, with its own GET /api/prefs (keeps this module free of prefs.ts).
let orderLoaded = false;
function loadOrder() {
  if (orderLoaded) return;
  orderLoaded = true;
  api.prefs().then(
    (r) => {
      orderW.set(r.machinesOrder ?? []);
      colorsW.set(cleanColors(r.machineColors));
    },
    () => (orderLoaded = false),
  );
}

/** Optimistic; hostd cleans the ids and its answer wins. A failure restores the old order and rethrows. */
export async function setMachinesOrder(ids: string[]): Promise<void> {
  const prev = get(orderW);
  orderW.set(ids);
  try {
    orderW.set((await api.putLayout({ machinesOrder: ids })).machinesOrder ?? ids);
  } catch (e) {
    orderW.set(prev);
    throw e;
  }
}

/** Sets one machine's colour (null resets it to the default). Optimistic; hostd's answer wins. A failure restores the old colours and rethrows. */
export async function setMachineColor(id: string, hex: string | null): Promise<void> {
  const norm = hex === null ? null : normalizeHex(hex);
  if (hex !== null && !norm) throw new Error(`invalid colour: ${hex}`);
  const prev = get(colorsW);
  const { [id]: _, ...rest } = prev;
  const next = norm ? { ...rest, [id]: norm } : rest;
  colorsW.set(next);
  try {
    colorsW.set(cleanColors((await api.putLayout({ machineColors: { [id]: norm } })).machineColors ?? next));
  } catch (e) {
    colorsW.set(prev);
    throw e;
  }
}

const PING_EVERY = 3_000;
const PING_TIMEOUT = 5_000;
const PING_KEEP = 60;
const pingsW = writable<Record<string, PingSample[]>>({});
/** Last 60 samples per machine, oldest first. */
export const pings: Readable<Record<string, PingSample[]>> = { subscribe: pingsW.subscribe };

/** Browser-to-machine time in ms; null on any failure. The opaque no-cors answer counts as success. */
async function directPing(url: string): Promise<number | null> {
  const ac = new AbortController();
  const to = setTimeout(() => ac.abort(), PING_TIMEOUT);
  const t0 = performance.now();
  try {
    // /api/ping answers 403 to a stranger: no CORP header (static files carry one), nothing read, no secret needed.
    await fetch(`${url}/api/ping`, { method: 'HEAD', mode: 'no-cors', cache: 'no-store', signal: ac.signal });
    return performance.now() - t0;
  } catch {
    return null;
  } finally {
    clearTimeout(to);
  }
}

const pinging = new Set<string>();
const addSample = (id: string, s: PingSample) => pingsW.update((p) => ({ ...p, [id]: [...(p[id] ?? []), s].slice(-PING_KEEP) }));

function pingTick() {
  if (get(hubW) !== true) return;
  const list = get(machinesW);
  pingsW.update((p) => Object.fromEntries(Object.entries(p).filter(([id]) => list.some((m) => m.id === id))));
  const t = Date.now();
  for (const m of list) {
    if (m.state !== 'ready' && m.state !== 'starting') addSample(m.id, { t, direct: null, hub: null });
    else if (!pinging.has(m.id)) {
      pinging.add(m.id);
      void directPing(m.url).then((direct) => {
        pinging.delete(m.id);
        const now = get(machinesW).find((x) => x.id === m.id);
        if (now) addSample(m.id, { t, direct, hub: now.rttMs });
      });
    }
  }
}

let pingUsers = 0;
let pingTimer: ReturnType<typeof setInterval> | undefined;
function pingRun() {
  clearInterval(pingTimer);
  if (document.hidden) return; // paused until visible again
  pingTick();
  pingTimer = setInterval(pingTick, PING_EVERY);
}

/** Sample latency every 3 s while a hub is known and the page is visible. Ref-counted like startMachines. */
export function startPings(): () => void {
  if (pingUsers++ === 0) {
    document.addEventListener('visibilitychange', pingRun);
    pingRun();
  }
  let done = false;
  return () => {
    if (done) return;
    done = true;
    if (--pingUsers === 0) {
      document.removeEventListener('visibilitychange', pingRun);
      clearInterval(pingTimer);
    }
  };
}
