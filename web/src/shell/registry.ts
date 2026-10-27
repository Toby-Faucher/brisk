import type { Component } from 'svelte';
import type { TagColor } from '../state/notes';

export type CommandItem = { id: string; label: string; hint?: string; icon?: string; tag?: { color: TagColor; symbol: string }; run: () => void };

export type ModuleManifest = {
  id: string;
  title: string;
  icon: string; // a name from ui/Icon.svelte
  color: string; // CSS color for the filled icon
  order: number;
  soon?: boolean; // greyed "soon" entry: no routes yet
  routes: { path: string; title: string }[];
  widget?: () => Promise<{ default: Component<any> }>;
  commands?: () => CommandItem[];
};

// A module is a folder with a module.ts: dropping one in is all it takes to appear in the shell.
const FOUND = import.meta.glob<{ default: ModuleManifest }>('../modules/*/module.ts', { eager: true });

export function loadModules(found: Record<string, { default: ModuleManifest }> = FOUND): ModuleManifest[] {
  return Object.values(found)
    .map((m) => m.default)
    .sort((a, b) => a.order - b.order);
}
