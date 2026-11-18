<script lang="ts">
  // A small LCD in the deck: host temperatures as LED bars and numbers. Read only; the real controls live in the deck.
  import { host } from '../../state/host';

  let { path }: { path: string } = $props();
  const SEGS = 10;
  const rows = $derived([
    ['CPU', $host?.cpuTempC ?? null],
    ['GPU', $host?.gpuTempC ?? null],
  ] as const);
  // 30 C is an empty bar, 90 C a full one
  const lit = (c: number | null) => (c == null ? 0 : Math.max(0, Math.min(SEGS, Math.round(((c - 30) / 60) * SEGS))));
</script>

{#if !/^\/stream/.test(path)}
  <div class="lcd" role="img" aria-label={rows.map(([k, c]) => `${k} ${c == null ? 'unknown' : `${Math.round(c)} degrees`}`).join(', ')}>
    {#each rows as [k, c] (k)}
      <div class="row" aria-hidden="true">
        <span class="k">{k}</span>
        <span class="bar">{#each { length: SEGS } as _, i}<i class:on={i < lit(c)} class:hot={i >= SEGS - 2}></i>{/each}</span>
        <span class="v">{c == null ? '--' : Math.round(c)}<small>C</small></span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .lcd {
    position: fixed;
    top: 12px;
    right: 252px;
    z-index: 31;
    display: grid;
    align-content: center;
    gap: 6px;
    height: 52px;
    padding: 0 12px;
    pointer-events: none;
    background: #23251f;
    border: 1px solid #0d0e0b;
    border-radius: 2px;
    box-shadow: 0 2px 5px rgba(0, 0, 0, 0.55) inset, 0 1px 0 #fffdf7;
    font-family: var(--mono);
    color: #cfe8c9;
  }
  .row { display: flex; align-items: center; gap: 8px; line-height: 1; }
  .k { width: 24px; font-size: 10px; letter-spacing: 0.12em; color: #b8c5b3; }
  .bar { display: flex; gap: 2px; }
  .bar i { width: 6px; height: 9px; background: #393c33; }
  .bar i.on { background: var(--accent); box-shadow: 0 0 5px color-mix(in srgb, var(--accent) 70%, transparent); }
  .bar i.on.hot { background: #ffd2bd; }
  .v { width: 34px; text-align: right; font-size: 12px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .v small { font-size: 9px; margin-left: 1px; color: #b8c5b3; }
  @media (max-width: 1180px) { .lcd { display: none; } }
  @media (prefers-reduced-motion: no-preference) {
    .bar i { transition: background-color 120ms steps(2, end); }
  }
</style>
