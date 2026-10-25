import type { Preset } from './api';

/** Stream controls fired from the command menu; the Stream module (Task 10) subscribes with onCommand. */
export type StreamCommand = { type: 'preset'; preset: Preset } | { type: 'stats' };

const subs = new Set<(c: StreamCommand) => void>();

export function onCommand(fn: (c: StreamCommand) => void): () => void {
  subs.add(fn);
  return () => void subs.delete(fn);
}

export function emitCommand(c: StreamCommand) {
  for (const fn of subs) fn(c);
}
