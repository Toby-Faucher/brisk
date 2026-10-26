<script lang="ts">
  // LayerChart is loaded only here (History imports this file dynamically), so no other page pays for it.
  // No Tailwind: LayerChart reads CSS variables (core.css only declares its cascade layers); ours are set below.
  import 'layerchart/core.css';
  import { AreaChart } from 'layerchart/svg';
  import { fmtClock, type ChartRow } from './history';

  let { rows }: { rows: ChartRow[] } = $props();

  const CHARTS = [
    { key: 'delay', label: 'Delay', unit: 'ms', color: 'var(--accent)', fmt: (v: number) => v.toFixed(0) },
    { key: 'fps', label: 'Frame rate', unit: 'fps', color: 'var(--c-stream)', fmt: (v: number) => v.toFixed(0) },
    { key: 'mbps', label: 'Bitrate', unit: 'Mbit/s', color: 'var(--c-hist)', fmt: (v: number) => v.toFixed(1) },
  ] as const;
  const mean = (k: keyof ChartRow) => rows.reduce((a, r) => a + r[k], 0) / (rows.length || 1);
</script>

<div class="charts">
  {#each CHARTS as c (c.key)}
    <figure class="chart">
      <figcaption>
        <span>{c.label}</span>
        <span class="mono">avg {c.fmt(mean(c.key))} {c.unit}</span>
      </figcaption>
      <div class="plot" role="img" aria-label="{c.label} over the session, average {c.fmt(mean(c.key))} {c.unit}">
        <AreaChart
          data={rows}
          x="t"
          series={[{ key: c.key, label: c.label, value: c.key, color: c.color }]}
          padding={{ left: 34, bottom: 22, top: 6, right: 8 }}
          props={{
            xAxis: { format: fmtClock, ticks: 6 },
            yAxis: { format: c.fmt, ticks: 4 },
            area: { fillOpacity: 0.18, line: { strokeWidth: 1.6 } },
            tooltip: { header: { format: fmtClock } },
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
