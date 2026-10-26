<script lang="ts">
  // LayerChart is loaded only here (History imports this file dynamically), so no other page pays for it.
  // No Tailwind: LayerChart reads CSS variables (core.css only declares its cascade layers); ours are set below.
  import 'layerchart/core.css';
  import { AreaChart } from 'layerchart/svg';
  import { createAttachmentKey } from 'svelte/attachments';
  import { drawIn, fadeIn } from '../../ui/motion';
  import { fmtClock, summarize, type ChartRow } from './history';

  let { rows }: { rows: ChartRow[] } = $props();

  const one = (v: number) => v.toFixed(1);
  // LayerChart spreads these onto its <path>s: the line draws in once per open, then the fill fades in under it
  const line = { [createAttachmentKey()]: drawIn };
  const fill = { [createAttachmentKey()]: fadeIn(250) };
  const CHARTS = [
    { key: 'delay', label: 'Delay', unit: 'ms', color: 'var(--accent)', fmt: (v: number) => v.toFixed(0) },
    { key: 'fps', label: 'Frame rate', unit: 'fps', color: 'var(--c-stream)', fmt: (v: number) => v.toFixed(0) },
    { key: 'mbps', label: 'Bitrate', unit: 'Mbit/s', color: 'var(--c-hist)', fmt: one },
  ] as const;
</script>

<div class="charts">
  {#each CHARTS as c (c.key)}
    {@const s = summarize(rows, c.key)!}
    <figure class="chart">
      <figcaption>
        <span>{c.label}</span>
        <span class="mono" aria-hidden="true">avg {c.fmt(s.avg)} {c.unit}</span>
        <!-- The chart's text alternative; the plot itself is decorative for screen readers. -->
        <span class="sr">
          {c.label} over the session: min {one(s.min)}, average {one(s.avg)}, max {one(s.max)} {c.unit}; the max at {fmtClock(s.peakT)}.
        </span>
      </figcaption>
      <div class="plot" aria-hidden="true">
        <AreaChart
          data={rows}
          x="t"
          series={[{ key: c.key, label: c.label, value: c.key, color: c.color }]}
          padding={{ left: 38, bottom: 24, top: 8, right: 10 }}
          props={{
            xAxis: { format: fmtClock, ticks: 6, tickLabelProps: { dy: 4 } },
            // the baseline needs no "0", which sat on the x axis's first label
            yAxis: { format: (v: number) => (v ? c.fmt(v) : ''), ticks: 3, tickLabelProps: { dx: -4 } },
            area: { fillOpacity: 0.18, line: { strokeWidth: 1.6, ...line }, ...fill },
            tooltip: { header: { format: fmtClock }, item: { format: one } },
          }}
        />
      </div>
    </figure>
  {/each}
</div>

<style>
  .charts { display: grid; gap: 14px; }
  .chart { margin: 0; background: color-mix(in srgb, var(--panel) 78%, transparent); border: 1px solid var(--line); border-radius: var(--r-card); padding: 14px 16px 10px; }
  figcaption { display: flex; justify-content: space-between; align-items: baseline; font-size: 12px; font-weight: 600; color: #c4cad6; margin-bottom: 6px; }
  figcaption .mono { font-weight: 400; color: var(--muted); }
  .sr { position: absolute; width: 1px; height: 1px; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .plot { height: 150px; }
  /* LayerChart's tokens, mapped to ours (its own defaults assume a Tailwind theme). */
  .plot :global(.lc-root-container),
  :global(.lc-tooltip-root) {
    --color-primary: var(--accent);
    --color-surface-100: var(--panel);
    --color-surface-200: var(--panel2);
    --color-surface-300: var(--line);
    --color-surface-content: var(--muted);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .plot :global(.lc-axis-tick-label) { font-size: 10.5px; stroke: none; fill: var(--dim); font-weight: 400; }
  :global(.lc-tooltip-root) { color: var(--fg); font-size: 12px; }
</style>
