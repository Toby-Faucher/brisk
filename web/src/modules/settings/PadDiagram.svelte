<script lang="ts">
  // Live 8BitDo Ultimate 2 diagram, true mm geometry from pad.svg.ts. Display only (aria-hidden), unless `select` is
  // given (the Emulator mappings editor): then the mapped pad buttons are real buttons, with the game function bound
  // to each drawn under it.
  import type { PadView } from './controllertest';
  import { BODY, BOUND_AT, BUMPERS, DPAD, DPAD_C, EXTRA_BOXES, EXTRA_CIRCLES, FACE, MIDDLE, PAD_LABEL, PAD_NAME_BY_ID, PADDLES, STICKS, TRIGGERS, USBC, VIEWBOX, type PadName } from './pad.svg';

  export type Select = {
    current: string | null;
    onpick: (pad: PadName) => void;
    labels: Partial<Record<PadName, string>>;
    hint?: string | null;
  };
  let { view, dz, trails, select }: { view: PadView; dz: number; trails: [string, string]; select?: Select } = $props();
  const on = (i?: number) => i !== undefined && !!view.buttons[i]?.pressed;
  const sticks = $derived([view.left, view.right]);
  const trig = $derived([view.lt, view.rt]);

  const nameOf = (id: string) => (select ? PAD_NAME_BY_ID[id] : undefined);
  const isSel = (id: string) => !!select && select.current === nameOf(id);
  const isFree = (id: string) => !!select && !!nameOf(id) && !select.labels[nameOf(id)!];
  /** Role, name and handlers of a mapped element; nothing at all outside select mode. */
  function ia(id: string) {
    const n = nameOf(id);
    if (!select || !n) return {};
    const bound = select.labels[n];
    const pick = () => select.onpick(n);
    return {
      role: 'button',
      tabindex: id === 'pad-down' || id === 'pad-left' || id === 'pad-right' ? -1 : 0, // the cross is one tab stop
      'aria-pressed': select.current === n ? ('true' as const) : ('false' as const),
      'aria-label': bound ? `${PAD_LABEL[n]}, does ${bound}. Press Enter to change.` : `${PAD_LABEL[n]}, not used. Press Enter to choose what it does.`,
      'data-pad': n,
      onclick: pick,
      onkeydown: (e: KeyboardEvent) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          pick();
        }
      },
    };
  }
  const svgAttrs = $derived(select ? { role: 'group', 'aria-label': 'Pad buttons' } : { 'aria-hidden': 'true' as const });
</script>

{#snippet bound(id: string)}
  {#if select && nameOf(id) && select.labels[nameOf(id)!]}
    {@const at = BOUND_AT[id]}
    <text class="bnd" x={at.x} y={at.y} text-anchor={at.anchor ?? 'middle'}>{select.labels[nameOf(id)!]}</text>
  {/if}
{/snippet}

<figure class="pd">
  <svg class="pad" class:pick={!!select} viewBox={VIEWBOX} focusable="false" {...svgAttrs}>
    <g id="pad-triggers">
      {#each TRIGGERS as t, k (t.id)}
        <g id={t.id} data-button={t.button} class:on={on(t.button)} class:sel={isSel(t.id)} class:free={isFree(t.id)} {...ia(t.id)}>
          <rect class="trk" x={t.x} y={t.y} width={t.w} height={t.h} rx="1.6" />
          {#if select}<rect class="hit" x={t.x} y={t.y} width={t.w} height={t.h} rx="1.6" />{/if}
          <rect class="fill" x={t.x} y={t.y} width={t.w * trig[k]} height={t.h} rx="1.6" />
          <text class="t" x={t.x + t.w / 2} y={t.y + t.h / 2}>{t.label} {(trig[k] * 100).toFixed(0)}%</text>
        </g>
        {@render bound(t.id)}
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
      <g id={b.id} data-button={b.button} class="key" class:on={on(b.button)} class:sel={isSel(b.id)} class:free={isFree(b.id)} {...ia(b.id)}><path d={b.d} />{#if select}<path class="hit" d={b.d} />{/if}<text class="t" x={b.lx} y={b.ly}>{b.label}</text>{@render bound(b.id)}</g>
    {/each}
    {#each EXTRA_CIRCLES as c (c.id)}<circle id={c.id} class="extra" cx={c.x} cy={c.y} r={c.r} />{/each}
    {#each EXTRA_BOXES.slice(2) as b (b.id)}<rect id={b.id} class="extra" x={b.x} y={b.y} width={b.w} height={b.h} rx="2" />{/each}
    {#each STICKS as s, k (s.id)}
      {@const v = sticks[k]}
      <g id={s.id} data-button={s.button} class="key stick" class:on={on(s.button)} class:sel={isSel(s.id)} class:free={isFree(s.id)} {...ia(s.id)}>
        <circle class="well" cx={s.x} cy={s.y} r={s.ring} />
        {#if select}<circle class="hit" cx={s.x} cy={s.y} r={s.ring} />{/if}
        <circle class="cap" cx={s.x + v.x * s.travel} cy={s.y + v.y * s.travel} r={s.r} />
        <!-- travel circle, deadzone ring and trail on top of the cap, in stick units (1 = full travel) -->
        <g transform="translate({s.x} {s.y}) scale({s.travel})">
          <circle class="travel" r="1" />
          <circle class="dzr" r={dz} />
          <polyline class="trail" points={trails[k]} />
        </g>
        <circle class="dot" class:out={!v.inDeadzone} cx={s.x + v.x * s.travel} cy={s.y + v.y * s.travel} r="0.9" />
        <text class="t" x={s.x + v.x * s.travel} y={s.y + v.y * s.travel + 3.6}>{s.label}</text>
        {@render bound(s.id)}
      </g>
    {/each}
    <g id="pad-dpad">
      <circle class="dish" cx={DPAD_C.x} cy={DPAD_C.y} r={DPAD_C.dish} />
      {#each DPAD as a (a.id)}
        <g id={a.id} data-button={a.button} class="key" class:on={on(a.button)} class:sel={isSel(a.id)} class:free={isFree(a.id)} {...ia(a.id)}><path d={a.d} />{#if select}<path class="hit" d={a.d} />{/if}<text class="t" x={a.lx} y={a.ly}>{a.label}</text></g>
      {/each}
      {@render bound('pad-up')}
    </g>
    {#each [...FACE, ...MIDDLE] as c (c.id)}
      <g id={c.id} data-button={c.button} class="key" class:on={on(c.button)} class:small={c.r < 4} class:sel={isSel(c.id)} class:free={isFree(c.id)} {...ia(c.id)}>
        <circle cx={c.x} cy={c.y} r={c.r} />
        {#if select}<circle class="hit" cx={c.x} cy={c.y} r={c.r} />{/if}
        <text class="t" x={c.x} y={c.r < 4 ? c.y - c.r - 2.2 : c.y}>{c.label}</text>
        {@render bound(c.id)}
      </g>
    {/each}
  </svg>
  {#if !select}
    <figcaption class="cap-note">8BitDo Ultimate 2 to scale. Faint: L4/R4, back paddles PL/PR and the extra face buttons, which are not exposed to browsers (Chrome's standard mapping reports only the 17 standard buttons).</figcaption>
  {:else if select.hint}
    <figcaption class="cap-note">{select.hint}</figcaption>
  {/if}
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
  .extra, .extra :is(rect, path) { fill: none; stroke: var(--dim); stroke-dasharray: 3 2.5; }
  /* Faint outlines only: the legend text in a .extra group stays at full --dim (4.5:1 on --panel2) */
  .extra:not(g) { opacity: .55; }
  g.extra :is(rect, path) { opacity: .3; } /* was .55 inside a .55 group */
  .t { font: 600 3.4px var(--font); fill: var(--muted); text-anchor: middle; dominant-baseline: central; pointer-events: none; stroke: none; }
  .key.on .t { fill: var(--accent-fg); }
  .key.small .t, .key.small.on .t { font-size: 2.9px; fill: var(--muted); }
  .key.small.on .t { fill: var(--accent); }
  .trk ~ .t { fill: var(--fg); font-size: 3px; stroke: var(--bg); stroke-width: 2.5px; paint-order: stroke; }
  #pad-lb .t, #pad-rb .t { font-size: 2.6px; }
  #pad-home .t { font-size: 2.5px; }
  .extra .t { fill: var(--dim); font-size: 2.8px; }
  /* select mode */
  .pick [role='button'] { cursor: pointer; }
  .pick [role='button']:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .pad .hit { fill: transparent; stroke: transparent; stroke-width: 6px; pointer-events: all; }
  .pad .sel > :is(path, circle, .trk):not(.hit, .dot, .cap) { stroke: var(--accent); stroke-width: 2.2px; }
  .pad .free:not(.sel) > :is(path, circle, .trk):not(.hit, .dot, .cap) { stroke: var(--dim); stroke-dasharray: 3 2.5; }
  .bnd { font: 600 2.6px var(--font); fill: var(--accent); dominant-baseline: central; pointer-events: none; stroke: none; }
  .cap-note { margin: 6px 0 0; font-size: 11.5px; color: var(--muted); text-align: center; }
</style>
