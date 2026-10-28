import type { Preset } from './api';
import type { CommandItem } from '../shell/registry';
import type { Machine } from './machines';
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

/** Ctrl+K entries in hub mode: switch to every other machine, start the ones that are off. */
export function machineEntries(
  ms: Machine[],
  selected: string | null,
  act: { select: (id: string) => void; start: (id: string) => void },
): CommandItem[] {
  return ms.flatMap((m) => [
    ...(m.id === selected
      ? []
      : [{ id: `machine:${m.id}`, label: `Switch to ${m.name}`, hint: 'Machine', icon: 'host', run: () => act.select(m.id) }]),
    ...(m.state === 'off' && m.canPower
      ? [{ id: `machine-start:${m.id}`, label: `Start ${m.name}`, hint: 'Machine', icon: 'host', run: () => act.start(m.id) }]
      : []),
  ]);
}
