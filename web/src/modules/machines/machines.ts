import { get } from 'svelte/store';
import { ApiError, type HostInfo } from '../../state/api';
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
