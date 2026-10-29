<script lang="ts">
  import { artSrc } from '../../state/art';
  import { flushSync, onMount, tick } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import type { TransitionBeforePreparationEvent } from 'astro:transitions/client';
  import { enter } from '../../shell/reveal';
  import { api, type SeriesPoint, type SessionRec } from '../../state/api';
  import { games, watchGames } from '../../state/games';
  import { findGame, rowKey } from '../../state/gameId';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import Tip from '../../ui/Tip.svelte';
  import { morph, skipMorph } from '../../ui/motion';
  import { DASH, fmtDuration } from '../../ui/format';
  import { chartRows, fmtWhen, listState, qualityLabel, sessionParam, sparkPath, statRows, filterSessions } from './history';

  type Detail = { session: SessionRec; series: SeriesPoint[] };

  let list = $state<(SessionRec & { spark?: number[] })[] | null>(null);
  let failed = $state(false);
  let q = $state('');
  let swapped = $state(false); // list/detail swap fades only after a user action (first load: page-in)
  let selected = $state<number | null>(null);
  let details = $state<Record<number, Detail | 'none'>>({});
  const shown = $derived(listState(list, failed));
  const visible = $derived(filterSessions(list ?? [], q));
  const detail = $derived(selected === null ? undefined : details[selected]); // only read while a session is selected
  const bad = new SvelteSet<string>(); // cover URLs that failed to load: the placeholder shows, in the list and the header
  const cover = (s: SessionRec) => {
    const c = findGame($games?.games ?? [], rowKey(s))?.cover;
    return c && !bad.has(c) ? c : undefined;
  };

  // The list rows carry their sparkline; only an opened session is fetched, once.
  const asked = new Set<number>();
  function load(id: number) {
    if (asked.has(id)) return;
    asked.add(id);
    api.session(id).then(
      (d) => (details[id] = d),
      () => (details[id] = 'none'),
    );
  }

  // List <-> detail. The session's cover is the shared element (motion.ts morph: a view transition naming only that
  // cover). `want` is the latest target, so updates that run late (a skipped transition still runs its update) agree.
  let want: number | null = null;
  function swap(id: number | null) {
    const from = selected;
    const moving = id ?? from; // whose cover moves: the opened one, or on the way back the one we leave
    want = id;
    morph('session-cover', () => (moving === null ? null : document.querySelector<HTMLElement>(`[data-cover="${moving}"]`)), () => {
      selected = want;
      if (want !== null) load(want);
      swapped = true;
      flushSync();
      // focus moves before the new snapshot is taken, so its scroll is already in it (instant: html scrolls smoothly)
      if (selected !== null) {
        window.scrollTo({ top: 0, behavior: 'instant' });
        document.querySelector<HTMLElement>('#session-title')?.focus({ preventScroll: true });
      } else if (from !== null) {
        const a = document.querySelector<HTMLElement>(`a.fill[data-session="${from}"]`);
        a?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
        a?.focus({ preventScroll: true });
      }
    });
  }

  function open(id: number | null) {
    // the router's state (same index) on our entries too, so coming back to one from another page is the router's
    // navigation (with null state it ignored it: another page's content under this URL); within History `keep` holds it
    history.pushState(history.state && { ...history.state, scrollX: 0, scrollY: 0 }, '', id === null ? '/history' : `/history?session=${id}`);
    swap(id);
  }

  function fetchSessions() {
    failed = false;
    api.sessions().then(
      (r) => {
        list = r.sessions;
        void tick().then(enter); // the cards arrive after hydration: give them the load entrance
      },
      () => (failed = true),
    );
  }

  onMount(() => {
    const unwatch = watchGames();
    selected = sessionParam(location.search);
    if (selected !== null) load(selected);
    fetchSessions();
    // Back/forward between History's own entries is ours. They all carry the router's state, so the router would
    // fetch and swap the whole page for each (no morph, focus lost). Its preparation event is the hook it offers: a
    // loader that never settles leaves that navigation pending (the next router navigation aborts it). The router's
    // popstate listener was registered first, so it runs (and fires the event) before our `pop` does.
    const here = location.pathname;
    const keep = (e: TransitionBeforePreparationEvent) => e.navigationType === 'traverse' && e.to.pathname === here && (e.loader = () => new Promise<void>(() => {}));
    const pop = () => location.pathname === here && swap(sessionParam(location.search));
    document.addEventListener('astro:before-preparation', keep);
    window.addEventListener('popstate', pop);
    return () => (skipMorph(), document.removeEventListener('astro:before-preparation', keep), window.removeEventListener('popstate', pop), unwatch());
  });
</script>

{#if selected !== null}
  {@const s = detail && detail !== 'none' ? detail.session : list?.find((r) => r.started === selected)}
  {@const c = s && cover(s)}
  <div class:swap-in={swapped}>
  <div class="top">
    <button class="btn ghost back" onclick={() => open(null)}><Icon name="chev" size={14} /> History</button>
  </div>
  <!-- fixed-size cover box: the header keeps its height when the image or the record arrives -->
  <div class="dh">
    {#if c}<img class="hcover" data-cover={selected} {@attach artSrc(c)} alt="" onerror={() => bad.add(c)} />{:else}<div class="hcover" data-cover={selected}></div>{/if}
    <div class="dt">
      <div class="dn">
        <h2 id="session-title" tabindex="-1">{s?.name ?? (detail === 'none' ? 'Session' : 'Loading…')}</h2>
        {#if s?.stats?.gate}<!-- svelte-ignore a11y_no_noninteractive_tabindex (focusable so keyboard users get its Tip too) -->
        <span class="gate {s.stats.gate}" role="note" tabindex="0">{s.stats.gate === 'pass' ? 'PASS' : 'FAIL'}</span><Tip label="M2 gate: bridge write p50 within 2 ms of the host's own" placement="bottom" />{/if}
      </div>
      {#if s}<p class="meta mono">{fmtWhen(s.started)} · {fmtDuration(s.started, s.ended)} · {qualityLabel(s)}</p>{/if}
    </div>
  </div>
  {#if detail === undefined}
    <p class="lab pad">Loading…</p>
  {:else if detail === 'none'}
    <p class="lab pad">This session is not on this host any more.</p>
  {:else}
    {#if detail.series.length > 1}
      {#await import('./Charts.svelte') then { default: Charts }}
        <Charts rows={chartRows(detail.series)} />
      {:catch}
        <p class="lab pad">The charts did not load.</p>
      {/await}
    {:else}
      <p class="lab pad">No samples were recorded for this session (the page records them while it shows the stream).</p>
    {/if}
    {@const rows = statRows(detail.session.stats)}
    <h3>Bridge report</h3>
    {#if rows.length}
      <dl class="stats">
        {#each rows as [k, v] (k)}<dt>{k}</dt><dd class="mono">{v}</dd>{/each}
      </dl>
      <p class="lab">Latencies are p50 / p99 on the host's clock (docs/m0-baseline.md).</p>
    {:else}
      <p class="lab">No report for this session.</p>
    {/if}
  {/if}
  </div>
{:else}
  <div class:swap-in={swapped}>
  <div class="top reveal"><h2>History</h2>{#if list?.length}<span class="lab">{list.length} sessions</span>
    <label class="search"><Icon name="search" size={16} /><input type="search" bind:value={q} onkeydown={(e) => e.key === 'Escape' && q && (e.preventDefault(), (q = ''))} placeholder="Search sessions" aria-label="Search sessions" /></label>{/if}</div>
  {#if shown === 'list'}
    {#if !visible.length}
      <div class="empty"><b>Nothing here</b><p class="lab">No session matches “{q.trim()}”.</p></div>
    {/if}
    <div class="grid">
      {#each visible as s (s.started)}
        {@const path = sparkPath(s.spark ?? [], 120, 28)}
        {@const c = cover(s)}
        <Card cols={1}>
          <a class="fill" href="/history?session={s.started}" data-session={s.started} onclick={(e) => (e.preventDefault(), open(s.started))} aria-label="{s.name}, {fmtWhen(s.started)}"></a>
          <div class="head">
            {#if c}<img class="cover" data-cover={s.started} {@attach artSrc(c)} alt="" onerror={() => bad.add(c)} />{:else}<div class="cover" data-cover={s.started}></div>{/if}
            <div class="txt"><b>{s.name}</b><div class="lab">{fmtWhen(s.started)}</div></div>
            {#if s.stats?.gate}<span class="gate {s.stats.gate}">{s.stats.gate === 'pass' ? 'PASS' : 'FAIL'}</span>{/if}
          </div>
          <div class="row mono"><span>{fmtDuration(s.started, s.ended)}</span><span>{qualityLabel(s)}</span></div>
          <svg class="spark" viewBox="0 0 120 28" preserveAspectRatio="none" aria-hidden="true">{#if path}<path d={path} />{/if}</svg>
        </Card>
      {/each}
    </div>
  {:else}
    <Card cols={2}>
      <h4><Icon name="chart" color="var(--c-hist)" size={16} /> History</h4>
      <div class="big">{DASH}</div>
      <div class="lab">
        {shown === 'empty' ? 'No streams yet. Play something and it shows up here.' : shown === 'error' ? 'History is not available here. Brisk could not reach the host agent.' : 'Loading…'}
      </div>
      {#if shown === 'error'}<button class="btn ghost" onclick={fetchSessions}>Try again</button>{/if}
    </Card>
  {/if}
  </div>
{/if}

<style>
  .top { padding: 0 2px 14px; border-bottom: 1px solid var(--line); display: flex; align-items: center; gap: 12px; }
  h2 { font-size: 18px; letter-spacing: -.02em; }
  h3 { font-size: 13px; font-weight: 600; margin: 22px 0 8px; color: #c4cad6; }
  .back { padding: 5px 10px 5px 6px; font-size: 13px; }
  .back :global(svg) { transform: scaleX(-1); }
  .meta { color: var(--muted); font-size: 12.5px; margin: 8px 0 0; }
  .dh { display: flex; align-items: center; gap: 18px; padding: 18px 2px 16px; }
  .hcover { width: 112px; height: 112px; border-radius: 18px; flex: 0 0 auto; object-fit: cover; background: linear-gradient(135deg, var(--panel2), var(--line)); }
  .dt { min-width: 0; }
  .dn { display: flex; align-items: center; gap: 12px; }
  h2:focus { outline: none; } /* focused by script on open (where a keyboard user lands); not a control */
  .search { margin-left: auto; display: flex; align-items: center; gap: 8px; padding: 6px 12px; border-radius: 11px; background: var(--panel); border: 1px solid var(--line); color: var(--muted); min-width: 210px; }
  .search:focus-within { border-color: #3a4152; }
  .search input { background: transparent; border: 0; outline: 0; color: var(--fg); font: inherit; width: 100%; }
  .empty { padding: 28px 2px; }
  .pad { padding: 16px 2px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: 14px; padding-top: 16px; }
  .fill { position: absolute; inset: 0; z-index: 1; }
  .head { display: flex; align-items: center; gap: 10px; }
  .cover { width: 34px; height: 34px; border-radius: 10px; flex: 0 0 auto; object-fit: cover; background: linear-gradient(135deg, var(--panel2), var(--line)); }
  .txt { min-width: 0; flex: 1; }
  .txt b { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row { display: flex; justify-content: space-between; gap: 8px; font-size: 12px; color: var(--muted); margin: 12px 0 6px; }
  .spark { width: 100%; height: 28px; display: block; }
  .spark path { fill: none; stroke: var(--accent); stroke-width: 1.4; vector-effect: non-scaling-stroke; stroke-linejoin: round; }
  .gate { font: 600 10.5px var(--mono); letter-spacing: .06em; padding: 2px 7px; border-radius: 999px; flex: 0 0 auto; }
  .gate.pass { color: var(--ok); background: color-mix(in srgb, var(--ok) 14%, transparent); }
  .gate.fail { color: var(--bad); background: color-mix(in srgb, var(--bad) 14%, transparent); }
  .big { font-size: 30px; font-weight: 700; /* rendered as 700 with the old static fonts; 650 would be a new, lighter weight */ letter-spacing: -.03em; margin: 6px 0 10px; }
  .stats { display: grid; grid-template-columns: auto 1fr; gap: 7px 22px; margin: 0 2px 8px; max-width: 560px; }
  dt { color: var(--muted); font-size: 13px; }
  dd { margin: 0; font-size: 13px; text-align: right; }
</style>
