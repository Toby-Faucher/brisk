<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { enter } from '../../shell/reveal';
  import { navigate } from 'astro:transitions/client';
  import { api, type Game } from '../../state/api';
  import { games, gamesFailed, refreshGames, watchGames } from '../../state/games';
  import { startGame, watchPrefs } from '../../state/prefs';
  import { notes, watchNotes } from '../../state/notes';
  import TagChip from './TagChip.svelte';
  import { toast } from '../../shell/Toasts.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { glideSwap, splitIn } from '../../ui/motion';
  import GameDetail from './GameDetail.svelte';
  import Hero from './Hero.svelte';
  import Shelf from './Shelf.svelte';
  import { cardClick, filterByTag, filterGames, searchGames, shelves, sortGames, type Filter, type Sort } from './library';

  const FILTERS: [Filter, string][] = [['all', 'All'], ['installed', 'Installed'], ['not-installed', 'Not installed'], ['recent', 'Recent']];
  const SORTS: [Sort, string][] = [['default', 'Default order'], ['az', 'A-Z'], ['most-played', 'Most played'], ['last-played', 'Last played']];
  let q = $state('');
  let sort = $state<Sort>('default');
  let f = $state<Filter>('all');
  let busy = $state<number | null>(null);
  // The game in the detail panel; looked up in the live list so a refresh updates it (running, installed) in place.
  let open = $state<Game | null>(null);
  const shownGame = $derived(open && ($games?.games.find((g) => g.appid === open!.appid) ?? open));

  // Tag filter: one tag at a time, on top of the filter chips and search. A tag that is gone stops filtering.
  let tagPick = $state<string | null>(null);
  const tagF = $derived(tagPick && $notes.tags[tagPick] ? tagPick : null);
  const usedTags = $derived(Object.entries($notes.tags).filter(([id]) => id === tagF || Object.values($notes.games).some((g) => g.tags.includes(id))));

  const s = $derived(shelves(sortGames(searchGames(filterByTag(filterGames($games?.games ?? [], f), $notes, tagF), q), sort)));
  const hero = $derived(q.trim() ? null : s.hero);

  // Skeleton -> content: when the games first arrive the hero, shelf headers and first cards get the load entrance
  // (once per mount; scrolling a shelf or paging it mounts cards that never animate).
  let arrived = false;
  $effect(() => {
    if ($games && !arrived) (arrived = true), void tick().then(enter);
  });

  // The hero title rises word by word once per page mount: not again when the hero remounts (search cleared, data refresh).
  let titled = false;
  const titleIn = (h: HTMLElement) => {
    if (!titled) return (titled = true), splitIn(h);
  };

  let body: HTMLElement | undefined = $state();
  let chips: HTMLElement | undefined = $state();

  // Plain swap: the results cross-fade quickly, at most once per 150 ms so typing never strobes.
  let fadedAt = 0;
  function fade() {
    const now = performance.now();
    if (!body || now - fadedAt < 150 || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    fadedAt = now;
    body.animate({ opacity: [0.3, 1] }, { duration: 140, easing: 'cubic-bezier(.2,.8,.2,1)' });
  }

  // Filter chips: a view transition. Cards in an open "Show all" grid that are on screen get a name (vt-<appid>) and
  // glide to their new place; virtualised shelf rows are never named. Names come off when it ends. Skipped (plain
  // swap) without support, with reduced motion or during the router's own transition. A second click skips ours first.
  // Not on search: measured in headless Chrome it made the slowest key 3-4x slower (docs/ui-notes.md, Platform features).
  let vt: ViewTransition | null = null;
  const named = new Set<HTMLElement>();
  const unname = () => (named.forEach((el) => (el.style.viewTransitionName = '')), named.clear());
  const nameCards = () => {
    const els = [...(body?.querySelectorAll<HTMLElement>('.grid [data-appid]') ?? [])].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.bottom > 0 && r.top < innerHeight;
    });
    for (const el of els.filter((e) => !named.has(e)).slice(0, 60 - named.size)) (el.style.viewTransitionName = `vt-${el.dataset.appid}`), named.add(el);
  };
  function pickFilter(id: Filter) {
    const update = () => glideSwap(chips, '[aria-pressed="true"]', () => (f = id));
    vt?.skipTransition();
    unname();
    if (!document.startViewTransition || matchMedia('(prefers-reduced-motion: reduce)').matches || document.documentElement.hasAttribute('data-astro-transition'))
      return void (update(), fade());
    nameCards();
    const t = (vt = document.startViewTransition({ update: async () => (update(), fade(), await tick(), nameCards()), types: ['library'] }));
    // skipped, aborted or a failed update: the promises reject; names come off either way (only the latest one's)
    const done = () => void (vt === t && ((vt = null), unname()));
    t.ready.catch(() => {});
    t.updateCallbackDone.catch(() => {});
    t.finished.then(done, done);
  }

  onMount(() => {
    const offGames = watchGames();
    const offPrefs = watchPrefs();
    const offNotes = watchNotes();
    return () => (offGames(), offPrefs(), offNotes());
  });

  async function run(g: Game, verb: string, task: () => Promise<unknown>) {
    if (busy != null) return;
    busy = g.appid;
    try {
      await task();
    } catch (e) {
      toast(`Could not ${verb} ${g.name}: ${(e as Error).message}`, { error: true });
    } finally {
      busy = null;
    }
  }

  function pick(g: Game) {
    if (cardClick(g).action === 'open') open = g;
  }

  const play = (g: Game) =>
    run(g, 'start', async () => {
      await startGame(g.appid, toast);
      await navigate('/stream');
    });

  const install = (g: Game) =>
    run(g, 'install', async () => {
      await api.install(g.appid);
      toast(`Installing ${g.name}. Steam will show progress`);
      void refreshGames();
    });
</script>

<div class="top reveal">
  <h2>Library</h2>
  <label class="search"><Icon name="search" size={16} /><input type="search" bind:value={q} oninput={fade} onkeydown={(e) => e.key === 'Escape' && q && ((q = ''), e.currentTarget.blur(), fade())} placeholder="Search games" aria-label="Search games" /></label>
</div>
<div class="filters reveal" role="group" aria-label="Filter games" bind:this={chips}>
  {#each FILTERS as [id, label] (id)}<button class="fchip" aria-pressed={f === id} onclick={() => pickFilter(id)}>{label}</button>{/each}
  <select class="sort" bind:value={sort} onchange={fade} aria-label="Sort games">{#each SORTS as [id, label] (id)}<option value={id}>{label}</option>{/each}</select>
</div>

{#if usedTags.length}
  <div class="filters" role="group" aria-label="Filter by tag">
    {#each usedTags as [id, t] (id)}<button class="fchip tagf" aria-pressed={tagF === id} onclick={() => ((tagPick = tagF === id ? null : id), fade())}><TagChip color={t.color} symbol={t.symbol} bare size={13} />{t.name}</button>{/each}
  </div>
{/if}

<div class="body" bind:this={body}>
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
    {#if hero}<Hero game={hero} busy={busy === hero.appid} onplay={play} ondetails={pick} titleIn={titleIn} />{/if}
    {#if s.fortnight.length && !q.trim() && f === 'all' && !tagF}<Shelf title="Played this fortnight" games={s.fortnight} onpick={pick} showFortnight />{/if}
    {#if s.installed.length}<Shelf title="Installed" games={s.installed} onpick={pick} />{/if}
    {#if s.notInstalled.length}<Shelf title="Not installed" games={s.notInstalled} onpick={pick} />{/if}
    {#if !s.installed.length && !s.notInstalled.length}
      <div class="empty"><b>Nothing here</b><p class="lab">{q.trim() ? `No game matches “${q.trim()}”.` : 'Try another filter.'}</p>
        <button class="btn ghost" onclick={() => ((q = ''), (f = 'all'), (tagPick = null))}>Clear search and filter</button>
      </div>
    {/if}
  {/if}
</div>

{#if shownGame}<GameDetail game={shownGame} busy={busy === shownGame.appid} onclose={() => (open = null)} onplay={play} oninstall={install} />{/if}

<style>
  .top { display: flex; align-items: center; gap: 12px; padding: 0 2px 14px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  h2 { font-size: 18px; letter-spacing: -.02em; }
  .search { display: flex; align-items: center; gap: 8px; padding: 6px 12px; border-radius: 11px; background: var(--panel); border: 1px solid var(--line); color: var(--muted); min-width: 210px; }
  .search:focus-within { border-color: #3a4152; }
  .search input { background: transparent; border: 0; outline: 0; color: var(--fg); font: inherit; width: 100%; }
  .filters { isolation: isolate; display: flex; gap: 6px; padding: 12px 0 0; flex-wrap: wrap; }
  .fchip { position: relative; padding: 5px 12px; border-radius: 999px; border: 1px solid var(--line); background: transparent; color: var(--muted); cursor: pointer; font-size: 13px; transition: color .2s var(--ease), border-color .2s var(--ease); }
  .fchip:hover { color: var(--fg); border-color: #3a4152; }
  /* the selected fill is a pseudo-element, so it can glide between chips (motion glideSwap) */
  .fchip[aria-pressed='true'] { border-color: transparent; color: var(--fg); }
  .fchip[aria-pressed='true']::before { content: ''; position: absolute; inset: -1px; z-index: -1; border-radius: inherit; background: color-mix(in srgb, var(--accent) 20%, transparent); border: 1px solid var(--accent); transform-origin: 0 0; }
  .tagf { display: inline-flex; align-items: center; gap: 6px; }
  .sort { margin-left: auto; padding: 5px 10px; border-radius: 999px; border: 1px solid var(--line); background: var(--panel); color: var(--muted); font: inherit; font-size: 13px; cursor: pointer; }
  .sort:hover { color: var(--fg); border-color: #3a4152; }
  .body { padding-top: 16px; }
  .banner { margin-bottom: 14px; padding: 9px 14px; border-radius: 12px; color: var(--warn); background: color-mix(in srgb, var(--warn) 12%, transparent); border: 1px solid color-mix(in srgb, var(--warn) 30%, transparent); }
  .empty { padding: 48px 0; display: grid; gap: 6px; justify-items: center; text-align: center; }
  .empty b { font-size: 16px; }
  .empty .btn { margin-top: 8px; }
  .skel { display: flex; gap: 14px; overflow: hidden; margin-top: 20px; }
  .skel div { flex: 0 0 138px; aspect-ratio: 2 / 3; border-radius: 16px; background: linear-gradient(100deg, var(--panel) 40%, var(--panel2) 50%, var(--panel) 60%) 0 0 / 300% 100%; animation: shimmer 1.4s linear infinite; }
  @keyframes shimmer { to { background-position: -150% 0; } }
</style>
