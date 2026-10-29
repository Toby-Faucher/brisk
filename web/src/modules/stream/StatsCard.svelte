<script lang="ts">
  import { DASH } from '../../ui/format';
  import type { StatsView } from './session';

  let { stats, path }: { stats: StatsView; path: string | null } = $props();

  const W = 272;
  const H = 38;
  // SVG, redrawn only when a sample lands (2 Hz): no canvas, no animation loop while streaming.
  const line = (d: number[], lo: number, hi: number) =>
    d.map((v, i) => `${((i / Math.max(1, d.length - 1)) * W).toFixed(1)},${(H - 3 - ((Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo)) * (H - 8)).toFixed(1)}`).join(' ');
  const last = (d: number[]) => (d.length ? d[d.length - 1] : null);
  const fmt = (v: number | null, digits: number) => (v === null ? DASH : v.toFixed(digits));
  const rows = $derived([
    { id: 'delay', label: 'Delay', unit: 'ms', data: stats.delay, lo: 0, hi: Math.max(40, ...stats.delay), digits: 1, color: '#60a5fa' },
    { id: 'fps', label: 'Frame rate', unit: 'fps', data: stats.fps, lo: 0, hi: Math.max(62, ...stats.fps), digits: 0, color: 'var(--accent)' },
    { id: 'bit', label: 'Bitrate', unit: 'Mbps', data: stats.bitrate, lo: 0, hi: Math.max(40, ...stats.bitrate), digits: 1, color: '#fbbf24' },
  ]);
</script>

<section class="stats glass" aria-label="Stream stats">
  <h5><span>Connection</span><span class="ok">● {path ?? 'Direct'}{stats.rttMs !== null ? ` · ${stats.rttMs.toFixed(0)} ms RTT` : ''}</span></h5>
  {#each rows as r (r.id)}
    <div class="gr">
      <span class="l">{r.label}</span>
      <span class="n">{fmt(last(r.data), r.digits)}<small>{r.unit}</small></span>
      <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" aria-hidden="true">
        <defs>
          <linearGradient id="g-{r.id}" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" style="stop-color:{r.color};stop-opacity:.4" />
            <stop offset="1" style="stop-color:{r.color};stop-opacity:0" />
          </linearGradient>
        </defs>
        {#if r.data.length > 1}
          <polygon points="0,{H} {line(r.data, r.lo, r.hi)} {W},{H}" fill="url(#g-{r.id})" />
          <polyline points={line(r.data, r.lo, r.hi)} fill="none" style="stroke:{r.color}" stroke-width="2.2" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
        {/if}
      </svg>
    </div>
  {/each}
  {#if stats.width}
    <p class="meta mono">{stats.width}×{stats.height} · {stats.codec ?? DASH} · {stats.hw === 'prefer-hardware' ? 'hw decode' : 'sw decode'}</p>
  {/if}
</section>

<style>
  .stats { position: fixed; left: 16px; top: 76px; width: 300px; padding: 14px; display: grid; gap: 10px; z-index: 32; animation: in .35s var(--ease); }
  @keyframes in { from { opacity: 0; transform: translateY(-8px) scale(.98); } }
  h5 { margin: 0; font-size: 11px; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); font-weight: 600; display: flex; justify-content: space-between; }
  .ok { color: var(--ok); }
  .gr { display: grid; grid-template-columns: 1fr auto; gap: 2px 10px; align-items: end; }
  .l { font-size: 12px; color: var(--muted); }
  .n { font-family: var(--mono); font-size: 19px; font-weight: 500; text-align: right; font-variant-numeric: tabular-nums; }
  .n small { font-size: 11px; color: var(--muted); margin-left: 2px; }
  svg { grid-column: 1 / 3; width: 100%; height: 38px; }
  .meta { margin: 0; font-size: 11.5px; color: var(--muted); }
</style>
