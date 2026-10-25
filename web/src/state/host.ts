import { writable } from 'svelte/store';
import { api, type HostInfo } from './api';

/** Active machine; null = unknown or unreachable (the UI shows dashes). Task 8 adds polling. */
export const host = writable<HostInfo | null>(null);

export async function refreshHost() {
  host.set(await api.host().catch(() => null));
}
