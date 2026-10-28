import { derived, get, writable, type Readable, type Writable } from 'svelte/store';
import { api, setRoute, type HostInfo } from './api';

export type MachineState = 'off' | 'starting' | 'stopping' | 'ready' | 'unreachable';
export type Machine = { id: string; name: string; state: MachineState; canPower: boolean; sunshine: 'ready' | 'busy' | 'down' | null; host: HostInfo | null };
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

function load(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}
const sel = writable<string | null>(load());
export const selected: Writable<string | null> = {
  set(id) {
    try {
      if (id === null) localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, id);
    } catch {}
    sel.set(id);
  },
  update: (fn) => selected.set(fn(get(sel))),
  subscribe: sel.subscribe,
};
export const selectedMachine: Readable<Machine | null> = derived([machinesW, sel], ([$m, $s]) => $m.find((m) => m.id === $s) ?? null);

// Machine-scoped api calls follow the selection, but only once a hub is known.
derived([hubW, sel], ([$h, $s]) => ($h ? $s : null)).subscribe(setRoute);

/** One fetch. Fixes the selection before publishing, so routed calls never see an unknown id. */
async function refresh(): Promise<void> {
  try {
    const { machines: list } = await api.machines();
    if (list.length && !list.some((m) => m.id === get(sel))) selected.set(list[0].id);
    machinesW.set(list);
    hubW.set(true);
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
