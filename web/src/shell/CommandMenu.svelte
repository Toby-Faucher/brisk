<script lang="ts">
  import { onMount } from 'svelte';
  import { Command, Dialog } from 'bits-ui';
  import { navigate } from 'astro:transitions/client';
  import { games, refreshGames } from '../state/games';
  import { api, type Preset } from '../state/api';
  import { emitCommand } from '../state/commands';
  import { attachShortcuts, rank } from './keys';
  import { loadModules, type CommandItem } from './registry';
  import { toast } from './Toasts.svelte';
  import { askQuit, stuck } from './QuitDialog.svelte';
  import { quitItems } from '../state/quit';
  import Icon from '../ui/Icon.svelte';

  const mods = loadModules().filter((m) => !m.soon);
  let open = $state(false);
  let query = $state('');

  const go = (path: string) => void navigate(path);
  const presets: Preset[] = ['fast', 'balanced', 'crisp'];

  const items = $derived.by<CommandItem[]>(() => [
    ...mods.flatMap((m) => m.routes.map((r) => ({ id: `route:${r.path}`, label: r.title, hint: 'Go to', icon: m.icon, run: () => go(r.path) }))),
    ...($games?.games ?? []).filter((g) => g.installed).map((g) => ({
      id: `game:${g.appid}`,
      label: `Launch ${g.name}`,
      icon: 'play',
      run: () => void api.launch(g.appid).catch((e) => toast(`Could not launch ${g.name}: ${e.message}`, { error: true })),
    })),
    ...quitItems($games?.games ?? [], $stuck, askQuit),
    { id: 'cmd:machine', label: 'Switch machine', hint: 'bawn', icon: 'host', run: () => go('/machines') },
    ...presets.map((p) => ({ id: `cmd:preset:${p}`, label: `Quality: ${p}`, icon: 'stream', run: () => { if (!emitCommand({ type: 'preset', preset: p })) toast(`Default quality: ${p}`); } })),
    { id: 'cmd:stats', label: 'Toggle stats', hint: 'Stream', icon: 'bolt', run: () => emitCommand({ type: 'stats' }) },
    ...mods.flatMap((m) => m.commands?.() ?? []),
  ]);
  const shown = $derived(rank(query, items).slice(0, 50));

  // The island remounts on every navigation: add on mount, remove on destroy.
  onMount(() => attachShortcuts(window, { open: () => (open = true), go }));

  $effect(() => {
    if (open) {
      query = '';
      if (!$games) void refreshGames();
    }
  });

  function pick(it: CommandItem) {
    open = false;
    it.run();
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content class="cm glass" aria-label="Command menu">
      <Dialog.Title class="sr">Command menu</Dialog.Title>
      <Command.Root shouldFilter={false} loop>
        <Command.Input bind:value={query} placeholder="Do something…" aria-label="Search actions" class="cm-in" />
        <Command.List class="cm-list">
          {#each shown as it (it.id)}
            <Command.Item value={it.id} onSelect={() => pick(it)} class="cm-it">
              {#if it.icon}<Icon name={it.icon} />{/if}
              <span>{it.label}</span>
              {#if it.hint}<kbd class="k">{it.hint}</kbd>{/if}
            </Command.Item>
          {:else}
            <Command.Empty class="cm-empty">Nothing matches</Command.Empty>
          {/each}
        </Command.List>
      </Command.Root>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  /* Portal content is outside this component's scope, so these are global by necessity. */
  :global(.cm-dim) { position: fixed; inset: 0; background: rgba(0, 0, 0, .35); z-index: 40; }
  :global(.cm) { position: fixed; left: 50%; top: 16%; transform: translateX(-50%); width: min(560px, calc(100vw - 32px)); padding: 8px; z-index: 41; --cm-r: 20px; --cm-pad: 8px; animation: cm-in .3s var(--ease); }
  @keyframes cm-in { from { opacity: 0; transform: translateX(-50%) translateY(-14px) scale(.97); } }
  :global(.cm .sr) { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  :global(.cm-in) { width: 100%; background: transparent; border: 0; outline: 0; color: var(--fg); font: inherit; font-size: 16px; padding: 12px 14px; border-bottom: 1px solid color-mix(in srgb, #fff 10%, transparent); box-sizing: border-box; }
  :global(.cm-list) { padding: 4px 0 0; display: grid; gap: 2px; max-height: 50vh; overflow-y: auto; overscroll-behavior: contain; }
  /* Chrome-only app: ::-webkit-scrollbar (the standard scrollbar-* properties would switch these off). The thumb is a pill inset from the edge via a transparent border. */
  :global(.cm-list::-webkit-scrollbar) { width: 12px; }
  :global(.cm-list::-webkit-scrollbar-track) { background: transparent; margin: 4px 0; }
  :global(.cm-list::-webkit-scrollbar-thumb) { background: color-mix(in srgb, #fff 16%, transparent); border: 4px solid transparent; background-clip: padding-box; border-radius: 999px; min-height: 36px; }
  :global(.cm-list::-webkit-scrollbar-thumb:hover) { background: color-mix(in srgb, var(--accent) 55%, transparent); background-clip: padding-box; }
  :global(.cm-list::-webkit-scrollbar-thumb:active) { background: var(--accent); background-clip: padding-box; }
  /* Concentric corners: inner radius = panel radius - panel padding (20px - 8px = 12px). */
  /* The global :focus-visible ring would draw a square teal box inside the rounded panel; the selected row is the focus cue here. */
  :global(.cm-in:focus-visible) { outline: none; box-shadow: none; }
  :global(.cm-it) { display: flex; align-items: center; gap: 12px; padding: 9px 12px; border-radius: calc(var(--cm-r) - var(--cm-pad)); cursor: pointer; }
  :global(.cm-it[data-selected]) { background: color-mix(in srgb, #fff 9%, transparent); }
  :global(.cm-it .k) { margin-left: auto; color: var(--dim); font-size: 12px; }
  :global(.cm-empty) { padding: 12px; color: var(--muted); }
  @media (prefers-reduced-motion: reduce) { :global(.cm) { animation: none; } }
</style>
