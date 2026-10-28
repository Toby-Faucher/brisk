<script lang="ts">
  /**
   * A small gauge ring. Colours come from `color` (the host's text colour) on the track and the arc; trouble levels turn it amber / red.
   * Decoration only: the caller keeps the label and value as real text.
   */
  let { pct, text, level = 'ok', size = 46 }: { pct: number; text: string; level?: 'ok' | 'warn' | 'bad' | 'static'; size?: number } = $props();

  const m = $derived(size / 2);
  const r = $derived(size / 2 - 5);
  const c = $derived(2 * Math.PI * r);
</script>

<svg class="ring {level}" width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true">
  <circle class="track" cx={m} cy={m} r={r} />
  <circle class="arc" cx={m} cy={m} r={r} transform="rotate(-90 {m} {m})" style:stroke-dasharray="{(Math.max(0, Math.min(100, pct)) / 100 * c).toFixed(1)} {c.toFixed(1)}" />
  <text x={m} y={m + 3.5} text-anchor="middle">{text}</text>
</svg>

<style>
  .ring { display: block; flex: 0 0 auto; color: inherit; }
  circle { fill: none; stroke-width: 5; }
  .track { stroke: currentColor; opacity: .22; }
  .arc { stroke: currentColor; stroke-linecap: round; }
  .static .arc { opacity: .7; }
  .warn .arc { stroke: var(--rw, var(--warn)); }
  .bad .arc { stroke: var(--rb, #f87171); }
  text { font: 600 10px var(--mono); fill: currentColor; }
  @media (prefers-reduced-motion: no-preference) {
    .arc { transition: stroke-dasharray .6s var(--ease), stroke .3s; }
  }
</style>
