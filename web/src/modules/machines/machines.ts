import { get } from 'svelte/store';
import { ApiError, type HostInfo } from '../../state/api';
import { DASH, fmtTemp } from '../home/home';
import { ensureReady, hub, selectedMachine, type Machine, type MachineState } from '../../state/machines';

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
