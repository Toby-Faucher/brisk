import { writable } from 'svelte/store';
import { api, type HostInfo } from './api';

/** Active machine; null = unknown or unreachable (the UI shows dashes). */
export const host = writable<HostInfo | null>(null);

export async function refreshHost() {
  host.set(await api.host().catch(() => null));
}

type Doc = EventTarget & { hidden: boolean };

/** Run `fn` now and every `every` ms, but only while the tab is visible. Returns the cleanup for onMount. */
export function pollVisible(fn: () => void, doc: Doc = document, every = 10_000): () => void {
  const tick = () => {
    if (!doc.hidden) fn();
  };
  tick();
  const id = setInterval(tick, every);
  doc.addEventListener('visibilitychange', tick);
  return () => {
    clearInterval(id);
    doc.removeEventListener('visibilitychange', tick);
  };
}

/** Refresh the host now and every `every` ms while visible. */
export const pollHost = (doc: Doc = document, every = 10_000) => pollVisible(() => void refreshHost(), doc, every);
