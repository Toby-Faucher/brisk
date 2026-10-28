<script lang="ts">
  import { areaPath, edgePath, niceMax, timeX, type PingSample, type Pt } from './latency';

  /**
   * The ping chart of a card header: no axes, labels or legend, drawn in the current text colour. You -> machine is the
   * filled layer, hub -> machine a fainter one; lost pings stay gaps. The readout next to it carries the numbers, so
   * this is decoration (aria-hidden). shortcut: redraws per sample instead of gliding like LatencyLayers, upgrade if it jars.
   */
  let { samples, height = 30 }: { samples: PingSample[]; height?: number } = $props();

  const uid = $props.id();
  let width = $state(280);
  const base = $derived(height - 1);
  const tNow = $derived(samples.at(-1)?.t ?? 0);
  const yMax = $derived(niceMax(Math.max(0, ...samples.flatMap((s) => [s.direct ?? 0, s.hub ?? 0]))));
  const line = (key: 'direct' | 'hub'): Pt[] => samples.map((s) => ({ x: timeX(s.t, tNow, width), y: s[key] === null ? null : base - (s[key]! / yMax) * (base - 3) }));
  const layers = $derived((['hub', 'direct'] as const).map((k) => ({ k, pts: line(k) })));
</script>

<div class="hs" bind:clientWidth={width}>
  <svg {width} {height} aria-hidden="true">
    <defs>
      <linearGradient id="{uid}-g" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="currentColor" stop-opacity=".4" />
        <stop offset="1" stop-color="currentColor" stop-opacity=".02" />
      </linearGradient>
    </defs>
    {#each layers as l (l.k)}
      <path class="area {l.k}" d={areaPath(l.pts, base)} fill="url(#{uid}-g)" />
      <path class="edge {l.k}" d={edgePath(l.pts)} />
    {/each}
  </svg>
</div>

<style>
  .hs { width: 100%; min-width: 0; color: inherit; }
  svg { display: block; overflow: hidden; max-width: 100%; }
  .edge { fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .hub { opacity: .4; }
</style>
