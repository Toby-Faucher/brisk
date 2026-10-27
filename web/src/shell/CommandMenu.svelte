<script lang="ts">
  import { onMount } from 'svelte';
  import { Command, Dialog } from 'bits-ui';
  import { navigate } from 'astro:transitions/client';
  import { games, refreshGames } from '../state/games';
  import { host } from '../state/host';
  import { api, type Preset } from '../state/api';
  import { emitCommand } from '../state/commands';
  import { attachShortcuts, rank } from './keys';
  import { loadModules, type CommandItem } from './registry';
  import { toast } from './Toasts.svelte';
  import { askQuit, stuck } from './QuitDialog.svelte';
  import { quitItems } from '../state/quit';
  import Icon from '../ui/Icon.svelte';
  import TagChip from '../modules/library/TagChip.svelte';
  import { notes, refreshNotes } from '../state/notes';
  import { openEntries, surpriseEntry, tagEntries } from './menuEntries';

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
    ...tagEntries($notes, $games?.games ?? [], go),
    ...[surpriseEntry($games?.games ?? [], go)].flatMap((e) => (e ? [e] : [])),
    ...quitItems($games?.games ?? [], $stuck, askQuit),
    { id: 'cmd:machine', label: 'Switch machine', hint: $host?.name ?? 'host', icon: 'host', run: () => go('/machines') },
    ...presets.map((p) => ({ id: `cmd:preset:${p}`, label: `Quality: ${p}`, icon: 'stream', run: () => { if (!emitCommand({ type: 'preset', preset: p })) toast(`Default quality: ${p}`); } })),
    { id: 'cmd:stats', label: 'Toggle stats', hint: 'Stream', icon: 'bolt', run: () => emitCommand({ type: 'stats' }) },
    ...mods.flatMap((m) => m.commands?.() ?? []),
  ]);
  // Open <game> uses Library's folded name search, so it is added after ranking rather than filtered by label.
  // It goes just before the Launch entries: Enter on a game name opens it, Launch stays one arrow away.
  const shown = $derived.by(() => {
    const ranked = rank(query, items);
    const at = ranked.findIndex((i) => i.id.startsWith('game:'));
    ranked.splice(at < 0 ? ranked.length : at, 0, ...openEntries($games?.games ?? [], query, go));
    return ranked.slice(0, 50);
  });

  // The island remounts on every navigation: add on mount, remove on destroy.
  onMount(() => attachShortcuts(window, { open: () => (open = true), go }));

  $effect(() => {
    if (open) {
      query = '';
      if (!$games) void refreshGames();
      void refreshNotes();
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
          <!-- the Viewport is what the input's aria-controls points at -->
          <Command.Viewport class="cm-vp">
          {#each shown as it (it.id)}
            <Command.Item value={it.id} onSelect={() => pick(it)} class="cm-it">
              {#if it.icon}<Icon name={it.icon} />{:else if it.tag}<TagChip color={it.tag.color} symbol={it.tag.symbol} bare size={16} />{/if}
              <span>{it.label}</span>
              {#if it.hint}<kbd class="k">{it.hint}</kbd>{/if}
            </Command.Item>
          {:else}
            <Command.Empty class="cm-empty">Nothing matches</Command.Empty>
          {/each}
          </Command.Viewport>
        </Command.List>
      </Command.Root>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  /* Portal content is outside this component's scope, so these are global by necessity. */
  :global(.cm-dim) { position: fixed; inset: 0; background: rgba(0, 0, 0, .35); z-index: 40; }
  :global(.cm) { position: fixed; left: 50%; top: 16%; transform: translateX(-50%); width: min(560px, calc(100vw - 32px)); padding: 8px; z-index: 41; --cm-r: 20px; --cm-pad: 8px; } /* enter/exit: base.css */
  :global(.cm .sr) { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  :global(.cm-in) { width: 100%; background: transparent; border: 0; outline: 0; color: var(--fg); font: inherit; font-size: 16px; padding: 12px 14px; border-bottom: 1px solid color-mix(in srgb, #fff 10%, transparent); box-sizing: border-box; }
  :global(.cm-list) { padding: 4px 0 0; max-height: 50vh; overflow-y: auto; overscroll-behavior: contain; }
  :global(.cm-vp) { display: grid; gap: 2px; }
  /* Scrollbar look is global (styles/base.css); the track only needs breathing room from the panel's rounded corners. */
  :global(.cm-list::-webkit-scrollbar-track) { margin: 4px 0; }
  /* Concentric corners: inner radius = panel radius - panel padding (20px - 8px = 12px). */
  /* The global :focus-visible ring would draw a square teal box inside the rounded panel; the selected row is the focus cue here. */
  :global(.cm-in:focus-visible) { outline: none; box-shadow: none; }
  :global(.cm-it) { display: flex; align-items: center; gap: 12px; padding: 9px 12px; border-radius: calc(var(--cm-r) - var(--cm-pad)); cursor: pointer; }
  :global(.cm-it[data-selected]) { background: color-mix(in srgb, #fff 9%, transparent); }
  :global(.cm-it .k) { margin-left: auto; color: var(--dim); font-size: 12px; }
  :global(.cm-it[data-selected] .k) { color: var(--muted); } /* --dim is under 4.5:1 on the selected row's fill */
  :global(.cm-empty) { padding: 12px; color: var(--muted); }
</style>
