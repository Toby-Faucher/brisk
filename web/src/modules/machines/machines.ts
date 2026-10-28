import { get } from 'svelte/store';
import { ApiError, type HostInfo } from '../../state/api';
import { DASH, fmtTemp, fmtUptime } from '../home/home';
import { ensureReady, hub, selectedMachine, type Machine, type MachineState, type PingSample } from '../../state/machines';

export type SoloMachine = { id: string; info: HostInfo };

/** One host today; the page renders a list so more machines slot in later. */
export const machinesFrom = (info: HostInfo): SoloMachine[] => [{ id: info.name.trim().toLowerCase() || 'host', info }];

export function restartError(e: unknown): string {
  if (e instanceof ApiError && e.status === 429) return 'Sunshine was restarted a moment ago, try again in a minute';
  return `Could not restart Sunshine: ${(e as Error).message}`;
}

export const STATE_LABEL: Record<MachineState, string> = { off: 'Off', starting: 'Starting…', stopping: 'Shutting down…', ready: 'Ready', unreachable: 'Not reachable' };

/** The StatusDot state for a machine: a ready one shows its Sunshine state. */
export function dotState(m: Pick<Machine, 'state' | 'sunshine'>): 'ready' | 'busy' | 'down' | 'off' {
  if (m.state === 'ready') return m.sunshine ?? 'ready';
  return m.state === 'starting' || m.state === 'stopping' ? 'busy' : m.state === 'unreachable' ? 'down' : 'off';
}

/** Why a card's Start / Shut down button is unavailable, or null when it works. */
export const powerWhy = (s: MachineState): string | null => (s === 'starting' ? 'Starting, hang on a moment' : s === 'stopping' ? 'Shutting down, hang on a moment' : null);

/** Seconds since `since` (unix ms), never negative. */
export const elapsedS = (since: number, now = Date.now()) => Math.max(0, Math.floor((now - since) / 1000));

/** Before Play / Stream: wake the selected machine when a hub has it off. A no-op in single-host mode. */
export async function readyFirst() {
  const m = get(selectedMachine);
  if (get(hub) === true && m && m.state !== 'ready') await ensureReady(m.id);
}

/** cpuPct is a percentage, 0..100 (as the hub sends it). */
export type Vm = { cpuPct: number; memUsedBytes: number; memTotalBytes: number };
/** One GPU / CPU / RAM line of a card; `n` is the number the text shows, so it can roll on later polls. */
export type StatRow = { label: string; text: string; n: number | null; fmt: (v: number) => string };

const GB = 1024 ** 3;
const gb = (v: number) => `${(v / GB).toFixed(1)} GB`;

/**
 * The hardware lines of a card. A hub shows only what it has: GPU with a GPU temperature, CPU as its temperature,
 * else the VM's load (plus RAM), else nothing. Single-host mode keeps today's fixed GPU and CPU lines (dash when null).
 */
export function statRows(info: Pick<HostInfo, 'gpuTempC' | 'cpuTempC'>, vm: Vm | null, hub: boolean): StatRow[] {
  const temp = (label: string, c: number | null): StatRow => ({ label, text: fmtTemp(c), n: c, fmt: fmtTemp });
  if (!hub) return [temp('GPU', info.gpuTempC), temp('CPU', info.cpuTempC)];
  const rows: StatRow[] = [];
  if (info.gpuTempC != null) rows.push(temp('GPU', info.gpuTempC));
  if (info.cpuTempC != null) rows.push(temp('CPU', info.cpuTempC));
  else if (vm) rows.push({ label: 'CPU load', text: `${Math.round(vm.cpuPct)} %`, n: vm.cpuPct, fmt: (v) => `${Math.round(v)} %` });
  if (vm) rows.push({ label: 'RAM', text: vm.memTotalBytes > 0 ? `${gb(vm.memUsedBytes)} / ${gb(vm.memTotalBytes)}` : DASH, n: null, fmt: String });
  return rows;
}

/** Pointer travel (px) up to which a press and release on a card still count as a click, not a drag or a text selection. */
export const TAP_PX = 5;
export const isTap = (down: { x: number; y: number } | null, up: { x: number; y: number }, max = TAP_PX) => !!down && Math.hypot(up.x - down.x, up.y - down.y) <= max;

/** The card swap to choreograph: only a change from one machine to another (not the first placement, not a repeat). */
/** A hub card rests dimmed when it is not the selected machine (and there is more than one to choose from). */
export const dimmed = (hub: boolean, count: number, selected: string | null, id: string) => hub && count > 1 && !!selected && selected !== id;

export const SUN_LABEL = { ready: 'Ready', busy: 'Streaming', down: 'Down' } as const;
export const networkText = (i: Pick<HostInfo, 'path' | 'relay'>) => (i.path === 'relay' ? `Relay${i.relay ? ` · ${i.relay}` : ''}` : i.path === 'direct' ? 'Direct' : DASH);

/** The chips under a card header; `running` is the already-worded Running value. */
export const chipData = (i: Pick<HostInfo, 'sunshine' | 'uptimeS' | 'path' | 'relay'>, running: string): { label: string; value: string }[] => [
  { label: 'Sunshine', value: SUN_LABEL[i.sunshine] },
  { label: 'Uptime', value: fmtUptime(i.uptimeS) },
  { label: 'Network', value: networkText(i) },
  { label: 'Running', value: running },
];

/** One ring of the card header. `pct` is 0..100; `static` is a neutral full ring (reserved, not load). */
export type Gauge = { key: 'cpu' | 'gpu' | 'ram'; label: string; pct: number; text: string; level: 'ok' | 'warn' | 'bad' | 'static' };
const lvl = (v: number, warn: number, bad: number): Gauge['level'] => (v >= bad ? 'bad' : v >= warn ? 'warn' : 'ok');
const pct100 = (v: number) => Math.max(0, Math.min(100, v));

/**
 * The rings of a hub card: CPU as temperature, else the VM's load, else none; GPU only with a GPU temperature; RAM only on a VM,
 * as a neutral full ring (a passthrough VM holds all its RAM, so used/total would read as load that is not there).
 */
export function gauges(host: Pick<HostInfo, 'cpuTempC' | 'gpuTempC'> | null, vm: Vm | null): Gauge[] {
  const out: Gauge[] = [];
  if (host?.cpuTempC != null) out.push({ key: 'cpu', label: 'CPU', pct: pct100(host.cpuTempC), text: `${Math.round(host.cpuTempC)}°`, level: lvl(host.cpuTempC, 70, 85) });
  else if (vm) out.push({ key: 'cpu', label: 'CPU', pct: pct100(vm.cpuPct), text: `${Math.round(vm.cpuPct)}%`, level: lvl(vm.cpuPct, 70, 90) });
  if (host?.gpuTempC != null) out.push({ key: 'gpu', label: 'GPU', pct: pct100(host.gpuTempC), text: `${Math.round(host.gpuTempC)}°`, level: lvl(host.gpuTempC, 75, 88) });
  if (vm) out.push({ key: 'ram', label: 'RAM', pct: 100, text: vm.memTotalBytes > 0 ? `${Math.round(vm.memTotalBytes / GB)}G` : DASH, level: 'static' });
  return out;
}

/** The header's ping readout from the newest sample: "10" ms direct, "1" ms hub, a dash when it did not answer. */
export function pingText(samples: PingSample[]): { direct: string; hub: string } {
  const s = samples.at(-1);
  const f = (n: number | null | undefined) => (n == null ? DASH : `${Math.round(n)} ms`);
  return { direct: f(s?.direct), hub: f(s?.hub) };
}

/**
 * Inline custom properties of a card: the machine colour, its header background, the text colour on it and the warn / bad ring
 * colours (darker under dark text so amber on yellow still reads). An idle card takes the page text and the default rings.
 */
export function headerVars(hue: string, on: string, live: boolean, bg: string): string {
  const dark = live && on === '#0b0d12';
  return `--h:${hue};--hbg:${bg};--on:${live ? on : 'var(--fg)'};--rw:${dark ? '#92400e' : 'var(--warn)'};--rb:${dark ? '#b91c1c' : '#f87171'}`;
}

/** Names of the PALETTE swatches, in order (ui/color.ts). */
export const COLOR_NAMES = ['blue', 'violet', 'pink', 'red', 'orange', 'yellow', 'green', 'teal'];
