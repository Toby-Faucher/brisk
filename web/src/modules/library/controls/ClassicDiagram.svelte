<script lang="ts" module>
  import type { Bindings, ControlId } from '../../../state/api';
  // Hotspot geometry from the approved mock-up: kind (rect or circle), x, y, w or r, h, tag x, tag y, tag anchor.
  type Hot = ['r' | 'c', number, number, number, number, number, number, 'start' | 'middle' | 'end'];
  export const HS: Record<ControlId, Hot> = {
    zl: ['r', 70, 20, 90, 26, 62, 37, 'end'],
    l: ['r', 70, 52, 90, 26, 62, 69, 'end'],
    zr: ['r', 400, 20, 90, 26, 498, 37, 'start'],
    r: ['r', 400, 52, 90, 26, 498, 69, 'start'],
    ls: ['c', 150, 150, 30, 0, 150, 198, 'middle'],
    dp: ['r', 138, 218, 64, 64, 170, 301, 'middle'],
    rs: ['c', 390, 250, 28, 0, 390, 296, 'middle'],
    x: ['c', 440, 116, 19, 0, 440, 86, 'middle'],
    y: ['c', 394, 160, 19, 0, 360, 164, 'end'],
    a: ['c', 486, 160, 19, 0, 522, 164, 'start'],
    b: ['c', 440, 204, 19, 0, 440, 238, 'middle'],
    minus: ['r', 220, 138, 40, 24, 240, 126, 'middle'],
    home: ['r', 264, 138, 40, 24, 284, 182, 'middle'],
    plus: ['r', 308, 138, 40, 24, 328, 126, 'middle'],
  };
</script>

<script lang="ts">
  import { CONTROLS } from './classic';

  let {
    bindings,
    custom,
    selected,
    listening,
    onselect,
    onlisten,
  }: {
    bindings: Bindings;
    custom: Record<ControlId, boolean>;
    selected: ControlId;
    listening: ControlId | null;
    onselect: (id: ControlId) => void;
    onlisten: (id: ControlId) => void;
  } = $props();

  let svg: SVGSVGElement;
  /** Put keyboard focus back on a control (after Esc, say). */
  export function focus(id: ControlId) {
    svg.querySelector<SVGElement>(`[data-hs="${id}"]`)?.focus();
  }

  const label = (id: ControlId, name: string) =>
    listening === id
      ? `${name}, listening. Press a button on your pad.`
      : custom[id]
        ? `${name}, custom. Press Enter to change.`
        : `${name}, bound to ${bindings[id]}. Press Enter to change.`;

  function key(e: KeyboardEvent, id: ControlId) {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    if (e.key === 'Enter' && selected === id) onlisten(id);
    else onselect(id);
  }
</script>

<svg bind:this={svg} class="pad" viewBox="0 0 560 320" role="group" aria-label="Classic Controller">
  <path
    class="body"
    d="M60 96 Q60 84 90 84 H470 Q500 84 500 96 Q540 220 520 280 Q505 312 470 296 Q440 280 420 270 H140 Q120 280 90 296 Q55 312 40 280 Q20 220 60 96Z"
  />
  {#each CONTROLS as c (c.id)}
    {@const h = HS[c.id]}
    {@const cx = h[0] === 'c' ? h[1] : h[1] + h[3] / 2}
    {@const cy = h[0] === 'c' ? h[2] : h[2] + h[4] / 2}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <g
      class="hs"
      class:sel={selected === c.id}
      class:listen={listening === c.id}
      data-hs={c.id}
      role="button"
      tabindex="0"
      aria-pressed={selected === c.id}
      aria-label={label(c.id, c.name)}
      onclick={() => onselect(c.id)}
      onkeydown={(e) => key(e, c.id)}
    >
      {#if h[0] === 'c'}
        <circle class="sh" cx={h[1]} cy={h[2]} r={h[3]} />
      {:else}
        <rect class="sh" x={h[1]} y={h[2]} width={h[3]} height={h[4]} rx={Math.min(h[4] / 2, 14)} />
      {/if}
      <text class="lb" x={cx} y={cy}>{c.label}</text>
      <text class="tg" class:custom={custom[c.id] && listening !== c.id} x={h[5]} y={h[6]} text-anchor={h[7]} aria-hidden="true">
        {listening === c.id ? 'press...' : custom[c.id] ? 'Custom' : bindings[c.id]}
      </text>
    </g>
  {/each}
</svg>

<style>
  .pad { width: 100%; min-width: 440px; height: auto; display: block; }
  .body { fill: var(--panel2); stroke: var(--line); stroke-width: 1.5; }
  .hs { cursor: pointer; outline: none; }
  .sh { fill: color-mix(in srgb, var(--fg) 9%, var(--panel2)); stroke: color-mix(in srgb, var(--fg) 30%, var(--panel2)); stroke-width: 1.5; transition: fill .15s, stroke .15s; }
  .lb { fill: var(--fg); font: 600 12px var(--font); text-anchor: middle; dominant-baseline: central; pointer-events: none; }
  .tg { fill: var(--accent); font: 500 14px var(--mono); pointer-events: none; }
  .tg.custom { fill: var(--warn); }
  .hs:hover .sh { fill: color-mix(in srgb, var(--fg) 16%, var(--panel2)); stroke: var(--muted); }
  .hs:focus-visible .sh { stroke: var(--accent); stroke-width: 3; }
  .hs.sel .sh { stroke: var(--accent); stroke-width: 2.5; fill: color-mix(in srgb, var(--accent) 18%, var(--panel2)); }
  .hs.listen .sh { stroke: var(--accent); fill: color-mix(in srgb, var(--accent) 35%, var(--panel2)); animation: pulse 1.1s ease-in-out infinite; }
  @keyframes pulse { 50% { stroke-width: 6; stroke-opacity: .35; } }
  @media (prefers-reduced-motion: reduce) { .hs.listen .sh { animation: none; } }
</style>
