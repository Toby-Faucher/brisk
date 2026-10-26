<script lang="ts">
  // Live 8BitDo Ultimate 2 diagram, true mm geometry from pad.svg.ts. Display only (aria-hidden).
  import type { PadView } from './controllertest';
  import { BODY, BUMPERS, DPAD, DPAD_C, EXTRA_BOXES, EXTRA_CIRCLES, FACE, MIDDLE, PADDLES, STICKS, TRIGGERS, USBC, VIEWBOX } from './pad.svg';

  let { view, dz, trails }: { view: PadView; dz: number; trails: [string, string] } = $props();
  const on = (i?: number) => i !== undefined && !!view.buttons[i]?.pressed;
  const sticks = $derived([view.left, view.right]);
  const trig = $derived([view.lt, view.rt]);
</script>

<figure class="pd">
  <svg class="pad" viewBox={VIEWBOX} aria-hidden="true" focusable="false">
    <g id="pad-triggers">
      {#each TRIGGERS as t, k (t.id)}
        <g id={t.id} data-button={t.button} class:on={on(t.button)}>
          <rect class="trk" x={t.x} y={t.y} width={t.w} height={t.h} rx="1.6" />
          <rect class="fill" x={t.x} y={t.y} width={t.w * trig[k]} height={t.h} rx="1.6" />
          <text class="t" x={t.x + t.w / 2} y={t.y + t.h / 2}>{t.label} {(trig[k] * 100).toFixed(0)}%</text>
        </g>
      {/each}
      {#each EXTRA_BOXES.slice(0, 2) as b (b.id)}
        <g id={b.id} class="extra"><rect x={b.x} y={b.y} width={b.w} height={b.h} rx="1.4" /><text class="t" x={b.x + b.w / 2} y={b.y + b.h / 2}>{b.label}</text></g>
      {/each}
    </g>
    <path id="pad-body" class="body" d={BODY} />
    <rect id={USBC.id} class="port" x={USBC.x} y={USBC.y} width={USBC.w} height={USBC.h} rx="1.6" />
    {#each PADDLES as p (p.id)}
      <g id={p.id} class="extra back"><path d={p.d} /><text class="t" x={p.lx} y={p.ly}>{p.label}</text></g>
    {/each}
    {#each BUMPERS as b (b.id)}
      <g id={b.id} data-button={b.button} class="key" class:on={on(b.button)}><path d={b.d} /><text class="t" x={b.lx} y={b.ly}>{b.label}</text></g>
    {/each}
    {#each EXTRA_CIRCLES as c (c.id)}<circle id={c.id} class="extra" cx={c.x} cy={c.y} r={c.r} />{/each}
    {#each EXTRA_BOXES.slice(2) as b (b.id)}<rect id={b.id} class="extra" x={b.x} y={b.y} width={b.w} height={b.h} rx="2" />{/each}
    {#each STICKS as s, k (s.id)}
      {@const v = sticks[k]}
      <g id={s.id} data-button={s.button} class="key stick" class:on={on(s.button)}>
        <circle class="well" cx={s.x} cy={s.y} r={s.ring} />
        <circle class="cap" cx={s.x + v.x * s.travel} cy={s.y + v.y * s.travel} r={s.r} />
        <!-- travel circle, deadzone ring and trail on top of the cap, in stick units (1 = full travel) -->
        <g transform="translate({s.x} {s.y}) scale({s.travel})">
          <circle class="travel" r="1" />
          <circle class="dzr" r={dz} />
          <polyline class="trail" points={trails[k]} />
        </g>
        <circle class="dot" class:out={!v.inDeadzone} cx={s.x + v.x * s.travel} cy={s.y + v.y * s.travel} r="0.9" />
        <text class="t" x={s.x + v.x * s.travel} y={s.y + v.y * s.travel + 3.6}>{s.label}</text>
      </g>
    {/each}
    <g id="pad-dpad">
      <circle class="dish" cx={DPAD_C.x} cy={DPAD_C.y} r={DPAD_C.dish} />
      {#each DPAD as a (a.id)}
        <g id={a.id} data-button={a.button} class="key" class:on={on(a.button)}><path d={a.d} /><text class="t" x={a.lx} y={a.ly}>{a.label}</text></g>
      {/each}
    </g>
    {#each [...FACE, ...MIDDLE] as c (c.id)}
      <g id={c.id} data-button={c.button} class="key" class:on={on(c.button)} class:small={c.r < 4}>
        <circle cx={c.x} cy={c.y} r={c.r} />
        <text class="t" x={c.x} y={c.r < 4 ? c.y - c.r - 2.2 : c.y}>{c.label}</text>
      </g>
    {/each}
  </svg>
  <figcaption class="cap-note">8BitDo Ultimate 2 to scale. Faint: L4/R4, back paddles PL/PR and the extra face buttons, which are not exposed to browsers (Chrome's standard mapping reports only the 17 standard buttons).</figcaption>
</figure>

<style>
  .pd { margin: 0; min-width: 0; }
  .pad { display: block; width: 100%; max-width: 560px; height: auto; margin: 0 auto; }
  .pad :global(*) { vector-effect: non-scaling-stroke; }
  .pad :is(path, rect, circle, polyline) { stroke-width: 1.5px; }
  .body { fill: var(--panel2); stroke: var(--line); }
  .port { fill: var(--bg); stroke: var(--line); }
  .key :is(path, circle) { fill: var(--bg); stroke: #343a48; }
  .key.on :is(path, circle) { fill: var(--accent); stroke: var(--accent); }
  .stick .well { fill: var(--bg); stroke: #343a48; }
  .stick .travel { fill: none; stroke: #2a2f3a; stroke-width: 1px; }
  .stick .cap { fill: #1d212b; stroke: #343a48; }
  .stick.on .well { fill: var(--bg); stroke: var(--accent); }
  .stick.on .cap { fill: var(--accent); stroke: var(--accent); }
  .dzr { fill: color-mix(in srgb, var(--warn) 14%, transparent); stroke: var(--warn); stroke-width: 1px; }
  .trail { fill: none; stroke: var(--accent); opacity: .6; stroke-linejoin: round; }
  .dot { fill: var(--muted); stroke: none; }
  .dot.out { fill: var(--accent); }
  .stick.on .dot { fill: var(--accent-fg); }
  .dish { fill: var(--bg); stroke: none; }
  .trk { fill: var(--bg); stroke: var(--line); }
  .fill { fill: var(--accent); opacity: .85; stroke: none; }
  .on .trk { stroke: var(--accent); }
  .extra, .extra :is(rect, path) { fill: none; stroke: var(--dim); stroke-dasharray: 3 2.5; opacity: .55; }
  .t { font: 600 3.4px var(--font); fill: var(--muted); text-anchor: middle; dominant-baseline: central; pointer-events: none; stroke: none; }
  .key.on .t { fill: var(--accent-fg); }
  .key.small .t, .key.small.on .t { font-size: 2.9px; fill: var(--muted); }
  .key.small.on .t { fill: var(--accent); }
  .trk ~ .t { fill: var(--fg); font-size: 3px; stroke: var(--bg); stroke-width: 2.5px; paint-order: stroke; }
  #pad-lb .t, #pad-rb .t { font-size: 2.6px; }
  #pad-home .t { font-size: 2.5px; }
  .extra .t { fill: var(--dim); font-size: 2.8px; }
  .cap-note { margin: 6px 0 0; font-size: 11.5px; color: var(--muted); text-align: center; }
</style>
