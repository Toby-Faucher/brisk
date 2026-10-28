<script lang="ts">
  // Settings > Emulators: pick an emulator, then one of its sections. What each section shows is listed in VIEW.
  import type { Component } from 'svelte';
  import { Tabs } from 'bits-ui';
  import EmulatorMappings from './EmulatorMappings.svelte';
  import Folders from './Folders.svelte';
  import { EMULATORS, emulatorOf, sectionOf, shownSections } from './emulators/registry';

  const VIEW: Record<string, Record<string, Component<{ focusKey?: number }>>> = { dolphin: { controllers: EmulatorMappings, folders: Folders } };
  let { emu, section, focusKey = 0, onpick }: { emu: string; section: string; focusKey?: number; onpick: (emu: string, section: string) => void } = $props();
  const e = $derived(emulatorOf(emu));
  const sec = $derived(sectionOf(e, section).id);
  const View = $derived(VIEW[e.id]?.[sec]);
</script>

<div class="emus">
  <Tabs.Root value={e.id} onValueChange={(v) => onpick(v, sectionOf(emulatorOf(v), null).id)}>
    <Tabs.List class="pick" aria-label="Emulator">
      {#each EMULATORS as x (x.id)}<Tabs.Trigger value={x.id} class="ptab">{x.name}</Tabs.Trigger>{/each}
    </Tabs.List>
    <Tabs.Content value={e.id}>
      <Tabs.Root value={sec} onValueChange={(v) => onpick(e.id, v)}>
        <Tabs.List class="pick sub" aria-label="{e.name} settings">
          {#each shownSections(e) as x (x.id)}<Tabs.Trigger value={x.id} class="ptab">{x.name}</Tabs.Trigger>{/each}
        </Tabs.List>
        <Tabs.Content value={sec}>{#if View}<View {focusKey} />{/if}</Tabs.Content>
      </Tabs.Root>
    </Tabs.Content>
  </Tabs.Root>
</div>

<style>
  .emus { padding-top: 14px; }
  .emus :global(.pick) { display: flex; gap: 6px; flex-wrap: wrap; }
  .emus :global(.sub) { margin-top: 10px; }
  .emus :global(.ptab) { padding: 6px 14px; border-radius: 999px; border: 1px solid var(--line); background: transparent; color: var(--muted); font: inherit; font-size: 13px; cursor: pointer; }
  .emus :global(.ptab[aria-selected='true']) { color: var(--fg); border-color: var(--accent); background: color-mix(in srgb, var(--accent) 20%, transparent); }
  .emus :global(.ptab:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
</style>
