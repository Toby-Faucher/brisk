<script lang="ts">
  import { DASH } from './format';
  import { motionAllowed } from './motion-base';
  import { areaPath, edgePath, lossXs, niceMax, stats, timeX, type Layer, type PingSample, type Pt } from './latency';

  /**
   * Layered ping chart: "You -> machine" (accent) over "Hub -> machine" (muted), area fills from the baseline on one time
   * axis, lost pings as gaps with ticks on the baseline. Purely presentational: the caller owns the samples (newest last).
   * The legend buttons hide and show a layer; the y-scale follows what is visible.
   */
  let { samples, height = 96 }: { samples: PingSample[]; height?: number } = $props();

  const uid = $props.id();
  const LAYERS: { key: Layer; name: string }[] = [
    { key: 'hub', name: 'Hub → machine' },
    { key: 'direct', name: 'You → machine' },
  ];
  const PAD = { t: 18, b: 8, r: 6 };
  let width = $state(280);
  let off = $state<Record<Layer, boolean>>({ direct: false, hub: false });
  let g: SVGGElement | undefined = $state();

  const base = $derived(height - PAD.b);
  const plotW = $derived(Math.max(1, width - PAD.r));
  const tNow = $derived(samples.at(-1)?.t ?? 0);
  const shown = $derived(LAYERS.filter((l) => !off[l.key]));
  const yMax = $derived(niceMax(Math.max(0, ...shown.flatMap((l) => samples.map((s) => s[l.key] ?? 0)))));
  const y = (ms: number) => base - (ms / yMax) * (base - PAD.t);
  const line = (key: Layer): Pt[] => samples.map((s) => ({ x: timeX(s.t, tNow, plotW), y: s[key] === null ? null : y(s[key]) }));
  const grid = $derived([yMax / 2, yMax]);
  const st = $derived({ direct: stats(samples, 'direct'), hub: stats(samples, 'hub') });
  const answered = $derived(samples.some((s) => (!off.direct && s.direct !== null) || (!off.hub && s.hub !== null)));
  const ms = (n: number | null) => (n === null ? DASH : `${Math.round(n)}`);
  const pct = (n: number) => (n === 0 ? '0%' : n < 1 ? '<1%' : `${Math.round(n)}%`);

  const say = (n: number | null) => (n === null ? 'no answer' : `${Math.round(n)} ms`);
  const label = $derived(
    samples.length
      ? `Latency: you to machine ${say(st.direct.now)}, hub to machine ${say(st.hub.now)}, over the last ${samples.length} pings`
      : 'Latency: waiting for the first ping',
  );

  // Screen readers hear a summary at most every ~10 s; the pings arrive every 3 s.
  let spoken = $state('');
  let spokeAt = 0;
  $effect(() => {
    const s = label;
    if (!spoken || Date.now() - spokeAt >= 10_000) [spoken, spokeAt] = [s, Date.now()];
  });

  // A new sample: the old drawing starts where it was and glides left by the time it advanced (translate only).
  let prevT = 0;
  $effect(() => {
    const t = tNow;
    const dt = t - prevT;
    prevT = t;
    if (g && dt > 0 && dt < 10_000 && samples.length > 1 && motionAllowed())
      g.animate({ translate: [`${(dt / 59_000) * plotW}px 0`, '0 0'] }, { duration: 700, easing: 'cubic-bezier(.2, .8, .2, 1)' });
  });
</script>

<div class="ll" bind:clientWidth={width}>
  <svg {width} {height} role="img" aria-label={label}>
    <defs>
      {#each LAYERS as l (l.key)}
        <linearGradient id="{uid}-{l.key}" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="var(--c-{l.key})" stop-opacity=".38" />
          <stop offset="1" stop-color="var(--c-{l.key})" stop-opacity=".02" />
        </linearGradient>
      {/each}
    </defs>
    {#each grid as v (v)}
      <line class="grid" x1="0" x2={width} y1={y(v)} y2={y(v)} />
      <text class="lbl" x="4" y={y(v) - 3}>{v} ms</text>
    {/each}
    <line class="base" x1="0" x2={width} y1={base} y2={base} />
    <g bind:this={g}>
      {#each LAYERS as l (l.key)}
        {#if !off[l.key] && samples.length}
          {@const pts = line(l.key)}
          <path class="area" d={areaPath(pts, base)} fill="url(#{uid}-{l.key})" />
          <path class="edge {l.key}" d={edgePath(pts)} />
          {#each lossXs(pts) as x}
            <line class="loss {l.key}" x1={x} x2={x} y1={base - 1} y2={base - 6} />
          {/each}
          {#if st[l.key].now !== null}
            <circle class="ring {l.key}" cx={plotW} cy={y(st[l.key].now!)} r="3.5" />
            <circle class="now {l.key}" cx={plotW} cy={y(st[l.key].now!)} r="3" />
          {/if}
        {/if}
      {/each}
    </g>
    {#if !samples.length}
      <text class="msg" x={width / 2} y={height / 2} text-anchor="middle">Waiting for the first ping…</text>
    {:else if !answered}
      <text class="msg" x={width / 2} y={height / 2} text-anchor="middle">No answer</text>
    {/if}
  </svg>

  <div class="rows">
    <span class="h"></span><span class="h">now</span><span class="h">min</span><span class="h">avg</span><span class="h">max</span><span class="h">loss</span>
    {#each LAYERS.toReversed() as l (l.key)}
      {@const s = st[l.key]}
      <button type="button" class="leg {l.key}" aria-pressed={!off[l.key]} onclick={() => (off[l.key] = !off[l.key])}>
        <i></i>{l.name}
      </button>
      <span>{ms(s.now)}</span><span>{ms(s.min)}</span><span>{ms(s.avg)}</span><span>{ms(s.max)}</span><span>{pct(s.lossPct)}</span>
    {/each}
  </div>
  <span class="sr" aria-live="polite">{spoken}</span>
</div>

<style>
  .ll { --c-direct: var(--accent); --c-hub: var(--muted); width: 100%; min-width: 0; }
  svg { display: block; overflow: hidden; max-width: 100%; }
  .grid { stroke: var(--line); stroke-width: 1; stroke-dasharray: 2 4; opacity: .7; }
  .base { stroke: var(--line); stroke-width: 1; }
  .lbl { fill: var(--dim); font: 9px var(--mono); paint-order: stroke; stroke: var(--panel); stroke-width: 3px; stroke-linejoin: round; }
  .msg { fill: var(--muted); font: 12px var(--font); }
  .edge { fill: none; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .edge.direct { stroke: var(--c-direct); }
  .edge.hub { stroke: var(--c-hub); opacity: .8; }
  .loss { stroke: var(--bad); stroke-width: 1.5; stroke-linecap: round; opacity: .85; }
  .now { stroke: var(--panel); stroke-width: 1.5; }
  .now.direct { fill: var(--c-direct); }
  .now.hub { fill: var(--c-hub); }
  .ring { fill: none; stroke-width: 1.5; opacity: 0; transform-box: fill-box; transform-origin: center; }
  .ring.direct { stroke: var(--c-direct); }
  .ring.hub { stroke: var(--c-hub); }
  @media (prefers-reduced-motion: no-preference) {
    .ring { animation: ll-pulse 2.4s var(--ease) infinite; }
  }
  @keyframes ll-pulse { 0% { opacity: .7; transform: scale(1); } 100% { opacity: 0; transform: scale(2.6); } }

  .rows { display: grid; grid-template-columns: minmax(0, 1fr) repeat(5, auto); gap: 2px 12px; align-items: center; margin-top: 6px; font: 11px var(--mono); color: var(--fg); font-variant-numeric: tabular-nums; }
  .rows > span { text-align: right; }
  .rows > .h { color: var(--dim); font-size: 10px; }
  .leg { all: unset; box-sizing: border-box; display: inline-flex; align-items: center; gap: 6px; min-width: 0; padding: 3px 4px; margin: -3px -4px; border-radius: 6px; font: 12px var(--font); color: var(--muted); cursor: pointer; }
  .leg i { flex: 0 0 auto; width: 8px; height: 8px; border-radius: 3px; background: var(--c-direct); }
  .leg.hub i { background: var(--c-hub); }
  .leg[aria-pressed='true'] { color: var(--fg); }
  .leg[aria-pressed='false'] i { background: none; box-shadow: inset 0 0 0 1.5px var(--dim); }
  .leg:hover { background: var(--panel2); }
  .leg:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .ll { position: relative; }
</style>
