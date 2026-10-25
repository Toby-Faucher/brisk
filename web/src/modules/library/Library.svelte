<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { navigate } from 'astro:transitions/client';
  import { api, type Game } from '../../state/api';
  import { games, gamesFailed, refreshGames, watchGames } from '../../state/games';
  import { settings } from '../../state/settings';
  import { toast } from '../../shell/Toasts.svelte';
  import Icon from '../../ui/Icon.svelte';
  import Hero from './Hero.svelte';
  import Shelf from './Shelf.svelte';
  import { filterGames, searchGames, shelves, type Filter } from './library';

  const FILTERS: [Filter, string][] = [['all', 'All'], ['installed', 'Installed'], ['not-installed', 'Not installed'], ['recent', 'Recent']];
  let q = $state('');
  let f = $state<Filter>('all');
  let busy = $state<number | null>(null);

  const s = $derived(shelves(searchGames(filterGames($games?.games ?? [], f), q)));
  const hero = $derived(q.trim() ? null : s.hero);

  onMount(() => watchGames());

  async function pick(g: Game) {
    if (busy != null) return;
    busy = g.appid;
    try {
      if (g.installed) {
        await api.startStream({ preset: get(settings).defaultPreset, appid: g.appid });
        await navigate('/stream');
      } else {
        await api.install(g.appid);
        toast(`Installing ${g.name}. Steam will show progress`);
      }
    } catch (e) {
      toast(`Could not ${g.installed ? 'start' : 'install'} ${g.name}: ${(e as Error).message}`, { error: true });
    } finally {
      busy = null;
    }
  }
</script>

<div class="top">
  <h2>Library</h2>
  <label class="search"><Icon name="search" size={16} /><input type="search" bind:value={q} placeholder="Search games" aria-label="Search games" /></label>
</div>
<div class="filters" role="group" aria-label="Filter games">
  {#each FILTERS as [id, label] (id)}<button class="fchip" aria-pressed={f === id} onclick={() => (f = id)}>{label}</button>{/each}
</div>

<div class="body">
  {#if $games && !$games.ownedAvailable}
    <div class="banner" role="status">Couldn't reach Steam. Showing installed games only for now.</div>
  {/if}

  {#if !$games}
    {#if $gamesFailed}
      <div class="empty">
        <b>Games are not available here</b>
        <p class="lab">Brisk could not reach the host agent on this machine. Start brisk-hostd and try again.</p>
        <button class="btn ghost" onclick={() => refreshGames()}>Try again</button>
      </div>
    {:else}
      <div class="skel" aria-label="Loading games">{#each { length: 7 } as _}<div></div>{/each}</div>
    {/if}
  {:else if !$games.games.length}
    <div class="empty"><b>No games yet</b><p class="lab">Games you own on Steam show up here.</p></div>
  {:else}
    {#if hero}<Hero game={hero} busy={busy === hero.appid} onpick={pick} />{/if}
    {#if s.installed.length}<Shelf title="Installed" games={s.installed} {busy} onpick={pick} />{/if}
    {#if s.notInstalled.length}<Shelf title="Not installed" games={s.notInstalled} {busy} onpick={pick} />{/if}
    {#if !s.installed.length && !s.notInstalled.length}
      <div class="empty"><b>Nothing here</b><p class="lab">{q.trim() ? `No game matches “${q.trim()}”.` : 'Try another filter.'}</p></div>
    {/if}
  {/if}
</div>

<style>
  .top { display: flex; align-items: center; gap: 12px; padding: 0 2px 14px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  h2 { font-size: 18px; letter-spacing: -.02em; }
  .search { display: flex; align-items: center; gap: 8px; padding: 6px 12px; border-radius: 11px; background: var(--panel); border: 1px solid var(--line); color: var(--muted); min-width: 210px; }
  .search:focus-within { border-color: #3a4152; }
  .search input { background: transparent; border: 0; outline: 0; color: var(--fg); font: inherit; width: 100%; }
  .filters { display: flex; gap: 6px; padding: 12px 0 0; flex-wrap: wrap; }
  .fchip { padding: 5px 12px; border-radius: 999px; border: 1px solid var(--line); background: transparent; color: var(--muted); cursor: pointer; font-size: 13px; transition: all .2s var(--ease); }
  .fchip:hover { color: var(--fg); border-color: #3a4152; }
  .fchip[aria-pressed='true'] { background: color-mix(in srgb, var(--accent) 20%, transparent); border-color: var(--accent); color: var(--fg); }
  .body { padding-top: 16px; }
  .banner { margin-bottom: 14px; padding: 9px 14px; border-radius: 12px; color: var(--warn); background: color-mix(in srgb, var(--warn) 12%, transparent); border: 1px solid color-mix(in srgb, var(--warn) 30%, transparent); }
  .empty { padding: 48px 0; display: grid; gap: 6px; justify-items: center; text-align: center; }
  .empty b { font-size: 16px; }
  .empty .btn { margin-top: 8px; }
  .skel { display: flex; gap: 14px; overflow: hidden; margin-top: 20px; }
  .skel div { flex: 0 0 138px; aspect-ratio: 2 / 3; border-radius: 16px; background: linear-gradient(100deg, var(--panel) 40%, var(--panel2) 50%, var(--panel) 60%) 0 0 / 300% 100%; animation: shimmer 1.4s linear infinite; }
  @keyframes shimmer { to { background-position: -150% 0; } }
</style>
