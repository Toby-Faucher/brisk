import type { Preset } from './api';
import { settings } from './settings';

/** Stream controls fired from the command menu; the Stream module (Task 10) subscribes with onCommand. */
export type StreamCommand = { type: 'preset'; preset: Preset } | { type: 'stats' };

const subs = new Set<(c: StreamCommand) => void>();

export function onCommand(fn: (c: StreamCommand) => void): () => void {
  subs.add(fn);
  return () => void subs.delete(fn);
}

/** False when nobody is listening (not on /stream); a preset then becomes the default quality instead. */
export function emitCommand(c: StreamCommand): boolean {
  if (subs.size === 0) {
    if (c.type === 'preset') settings.update((v) => ({ ...v, defaultPreset: c.preset }));
    return false;
  }
  for (const fn of subs) fn(c);
  return true;
}
