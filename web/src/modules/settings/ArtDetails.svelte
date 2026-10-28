<script lang="ts">
  // Emulators.svelte passes every view a focusKey; this one has nothing to focus.
  const _props: { focusKey?: number } = $props();
  // Settings > Emulators > Dolphin > Art and details: the games whose title matched several things, one question each.
  import { onMount } from 'svelte';
  import { api, ApiError, type ArtPending, type ArtQueue } from '../../state/api';
  import { refreshGames } from '../../state/games';
  import { selected } from '../../state/machines';
  import { toast } from '../../shell/Toasts.svelte';

  let queue = $state<ArtQueue | null>(null);
  let old = $state(false);
  let failed = $state(false);
  let busy = $state('');
  let gen = 0;

  async function load() {
    const g = ++gen;
    queue = null;
    old = failed = false;
    try {
      const q = await api.artPending();
      if (g === gen) queue = q;
    } catch (e) {
      if (g !== gen) return;
      if (e instanceof ApiError && e.status === 404) old = true; // an old hostd has no such route
      else failed = true;
    }
  }
  onMount(() => selected.subscribe(() => void load()));

  async function answer(item: ArtPending, pick: { candidateId: number } | { none: true }) {
    if (busy) return;
    busy = item.id;
    try {
      await api.artAnswer(item.id, pick);
      if (queue) queue = { ...queue, items: queue.items.filter((x) => x.id !== item.id) };
      if ('none' in pick) toast(`Okay. Brisk will not ask about ${item.title} again.`);
      else {
        toast(`Getting art for ${item.title}`);
        setTimeout(() => void refreshGames(), 5000);
      }
    } catch {
      toast('Could not save that choice. Try again.', { error: true });
    }
    busy = '';
  }
</script>

<section class="fold" aria-label="Art and details">
  <h3>Art and details</h3>
  <p>Brisk looks up art for each game once, by its title, and keeps it on the machine. Nothing is fetched for a game it is unsure about until you choose here. This list is for the machine you picked.</p>
  {#if old}
    <p class="lab">This machine's Brisk is too old for art. Update it first.</p>
  {:else if failed}
    <p class="err">Could not load the list.</p>
  {:else if queue}
    {#if !queue.hasKey}<p class="lab">Art needs a SteamGridDB key on the machine.</p>{/if}
    {#each queue.items as item (item.id)}
      <div class="q" role="group" aria-label="Choose art for {item.title}">
        <h4>{item.title}</h4>
        <ul class="cands">
          {#each item.candidates as c (c.id)}
            <li>
              <b>{c.name}</b>
              <span class="lab">{c.year ?? 'Year unknown'}{c.verified ? ' · verified' : ''}</span>
              <button class="btn ghost" aria-disabled={!!busy} aria-label="Choose {c.name} for {item.title}" onclick={() => answer(item, { candidateId: c.id })}>Choose</button>
            </li>
          {/each}
        </ul>
        <button class="btn ghost" aria-disabled={!!busy} onclick={() => answer(item, { none: true })}>None of these</button>
      </div>
    {:else}
      <p class="lab">No games are waiting for a choice.</p>
    {/each}
    <p class="credit">Art from <a class="link" href="https://www.steamgriddb.com" target="_blank" rel="noopener noreferrer">SteamGridDB</a>.</p>
  {:else}
    <p class="lab" role="status">Loading…</p>
  {/if}
</section>

<style>
  .fold { margin-top: 14px; padding: 16px; border-radius: 16px; background: var(--panel); border: 1px solid var(--line); display: grid; gap: 4px; max-width: 640px; }
  h3 { margin: 0 0 6px; font-size: 15px; }
  h4 { margin: 8px 0 4px; font-size: 14px; }
  p { margin: 0 0 10px; }
  .q { padding-top: 6px; border-top: 1px solid var(--line); }
  .cands { list-style: none; margin: 0 0 8px; padding: 0; display: grid; gap: 8px; }
  .cands li { display: grid; grid-template-columns: 1fr auto; gap: 2px 10px; padding: 10px 12px; border-radius: 12px; border: 1px solid var(--line); align-items: center; }
  .cands li .lab { grid-column: 1; }
  .cands li button { grid-column: 2; grid-row: 1 / span 2; }
  .credit { font-size: 12px; color: var(--muted); }
  .err { color: var(--bad, #e66); }
</style>
