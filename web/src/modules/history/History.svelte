<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { enter } from '../../shell/reveal';
  import { api, type SeriesPoint, type SessionRec } from '../../state/api';
  import { games, watchGames } from '../../state/games';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { DASH, fmtDuration } from '../home/home';
  import { chartRows, fmtWhen, listState, qualityLabel, sessionParam, sparkPath, statRows } from './history';

  type Detail = { session: SessionRec; series: SeriesPoint[] };

  let list = $state<(SessionRec & { spark?: number[] })[] | null>(null);
  let failed = $state(false);
  let swapped = $state(false); // list/detail swap fades only after a user action (first load: page-in)
  let selected = $state<number | null>(null);
  let details = $state<Record<number, Detail | 'none'>>({});
  const shown = $derived(listState(list, failed));
  const detail = $derived(selected === null ? undefined : details[selected]); // only read while a session is selected
  const cover = (appid: number | null) => $games?.games.find((g) => g.appid === appid)?.cover;

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

  function open(id: number | null, push = true) {
    selected = id;
    if (id !== null) load(id);
    if (push) history.pushState(null, '', id === null ? '/history' : `/history?session=${id}`);
    swapped = true;
    window.scrollTo({ top: 0, behavior: 'instant' });
  }

  onMount(() => {
    const unwatch = watchGames();
    selected = sessionParam(location.search);
    if (selected !== null) load(selected);
    api.sessions().then(
      (r) => {
        list = r.sessions;
        void tick().then(enter); // the cards arrive after hydration: give them the load entrance
      },
      () => (failed = true),
    );
    // Our entries carry no state, so Astro's router leaves them to us.
    const pop = () => ((selected = sessionParam(location.search)), (swapped = true));
    window.addEventListener('popstate', pop);
    return () => (window.removeEventListener('popstate', pop), unwatch());
  });
</script>

{#if selected !== null}
  {@const s = detail && detail !== 'none' ? detail.session : list?.find((r) => r.started === selected)}
  <div class:swap-in={swapped}>
  <div class="top">
    <button class="btn ghost back" onclick={() => open(null)}><Icon name="chev" size={14} /> History</button>
    {#if s}
      <h2>{s.name}</h2>
      {#if s.stats?.gate}<span class="gate {s.stats.gate}" title="M2 gate: bridge write p50 within 2 ms of the host's own">{s.stats.gate === 'pass' ? 'PASS' : 'FAIL'}</span>{/if}
    {/if}
  </div>
  {#if s}
    <p class="meta mono">{fmtWhen(s.started)} · {fmtDuration(s.started, s.ended)} · {qualityLabel(s)}</p>
  {/if}
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
      <p class="lab">Latencies are p50 / p99 on bawn's clock (docs/m0-baseline.md).</p>
    {:else}
      <p class="lab">No report for this session.</p>
    {/if}
  {/if}
  </div>
{:else}
  <div class:swap-in={swapped}>
  <div class="top reveal"><h2>History</h2>{#if list?.length}<span class="lab">{list.length} sessions</span>{/if}</div>
  {#if shown === 'list'}
    <div class="grid">
      {#each list ?? [] as s (s.started)}
        {@const path = sparkPath(s.spark ?? [], 120, 28)}
        {@const c = cover(s.appid)}
        <Card cols={1}>
          <a class="fill" href="/history?session={s.started}" onclick={(e) => (e.preventDefault(), open(s.started))} aria-label="{s.name}, {fmtWhen(s.started)}"></a>
          <div class="head">
            {#if c}<img class="cover" src={c} alt="" />{:else}<div class="cover"></div>{/if}
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
  .meta { color: var(--muted); font-size: 12.5px; margin: 12px 2px 14px; }
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
  .big { font-size: 30px; font-weight: 650; letter-spacing: -.03em; margin: 6px 0 10px; }
  .stats { display: grid; grid-template-columns: auto 1fr; gap: 7px 22px; margin: 0 2px 8px; max-width: 560px; }
  dt { color: var(--muted); font-size: 13px; }
  dd { margin: 0; font-size: 13px; text-align: right; }
</style>
