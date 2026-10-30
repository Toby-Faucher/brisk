<script lang="ts">
  // The top HUD strip for the Overworld style: heart containers, a gem counter, a dotted dungeon map and an old sage
  // with a hint for each screen. Decorative only (aria-hidden, no pointer events). Never mounted on /stream.
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  const ROOMS: [string, string, number, number][] = [
    ['/', 'Home', 0, 0],
    ['/library', 'Library', 1, 0],
    ['/stream', 'Stream', 2, 0],
    ['/machines', 'Machines', 0, 1],
    ['/history', 'History', 1, 1],
    ['/settings', 'Settings', 2, 1],
  ];
  const here = $derived(ROOMS.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p)))?.[0] ?? '/');
  // meadow outside, stone inside
  const zone = $derived(here === '/' || here === '/library' ? 'mdw' : 'dun');

  const HINTS: Record<string, string> = {
    '/': 'Welcome, traveler. Your games are resting in the library.',
    '/library': 'Choose a game and press Enter. Heroes do not read manuals.',
    '/machines': 'Each room is a machine. The dots are the hallways.',
    '/history': 'Old sessions are carved in these walls. Read them well.',
    '/settings': 'Tweak what you like. The reset buttons are brave ones.',
  };
  const hint = $derived(HINTS[here] ?? HINTS['/']);
  let shout = $state('');
  const say = $derived(shout || hint);

  // Hearts: three and a half of five. They throb once on every screen change.
  const HEARTS = [100, 100, 100, 50, 0];

  // Gems: every click anywhere is a small find. The count lives for the tab session.
  let gems = $state(0);
  let pop = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  onMount(() => {
    try {
      gems = Number(sessionStorage.getItem('ow-gems')) || 0;
    } catch {
      /* private window: start at zero */
    }
    const add = () => {
      gems += 1;
      try {
        sessionStorage.setItem('ow-gems', String(gems));
      } catch {
        /* ignore */
      }
      pop = false;
      requestAnimationFrame(() => (pop = true));
      if (gems % 25 === 0) {
        shout = 'Another 25. You are a dragon of the hoard.';
        clearTimeout(timer);
        timer = setTimeout(() => (shout = ''), 4000);
      }
    };
    addEventListener('pointerdown', add, true);
    return () => (removeEventListener('pointerdown', add, true), clearTimeout(timer));
  });
  const count = $derived(String(gems).padStart(3, '0'));
</script>

{#if !stream}
  <div class="ow-zone" data-z={zone} data-p={here} aria-hidden="true"></div>
  <div class="ow-hud" aria-hidden="true">
    <div class="ow-hearts">
      <span class="ow-k">Hearts</span>
      {#each HEARTS as pct, i (i)}
        <svg viewBox="0 0 7 6" width="21" height="18" shape-rendering="crispEdges">
          <path class="e" d="M1 0h2v1h1V0h2v1h1v2h-1v1h-1v1h-1v1h-1V5H2V4H1V3H0V1h1z" />
          <path class="f" style:clip-path="inset(0 {100 - pct}% 0 0)" d="M1 0h2v1h1V0h2v1h1v2h-1v1h-1v1h-1v1h-1V5H2V4H1V3H0V1h1z" />
        </svg>
      {/each}
    </div>
    <div class="ow-gems" class:pop>
      <svg viewBox="0 0 7 8" width="16" height="18" shape-rendering="crispEdges">
        <path d="M2 0h3l2 2-3.5 6L0 2z" fill="#3fd07a" />
        <path d="M2 0h3l-1.5 2zM0 2h7l-3.5 6z" fill="#7bf0a6" opacity=".55" />
        <path d="M0 2h7" stroke="#16603a" stroke-width=".5" />
      </svg>
      <span class="ow-x">x</span><b>{count}</b>
    </div>
    <div class="ow-map">
      <svg viewBox="0 0 66 29" width="55" height="24" shape-rendering="crispEdges">
        <path d="M13 5h10M36 5h10M13 21h10M36 21h10M7 10v6M30 10v6M53 10v6" stroke="#c9b78f" stroke-width="1.5" stroke-dasharray="2 2" />
        {#each ROOMS as [p, name, c, r] (p)}
          <rect class="room" class:on={p === here} x={c * 23 + 1} y={r * 16 + 1} width="12" height="9" data-room={name} />
        {/each}
      </svg>
    </div>
    <div class="ow-sage">
      <svg viewBox="0 0 12 12" width="30" height="30" shape-rendering="crispEdges">
        <path d="M3 0h6v1h1v2h1v9H1V3h1V1h1z" fill="#6c4fb0" />
        <path d="M3 3h6v4H3z" fill="#e9c9a0" />
        <path d="M3 7h6v1h1v3H2V8h1z" fill="#f4f1e8" />
        <path d="M4 4h1v1H4zM7 4h1v1H7z" fill="#2e2010" />
        <path d="M3 1h6v1H3z" fill="#8b6fd0" />
      </svg>
      {#key say}<p><span>{say}</span></p>{/key}
    </div>
  </div>
  <div class="ow-hedge" data-z={zone} aria-hidden="true"></div>
{/if}

<style>
  .ow-zone, .ow-hedge { position: fixed; pointer-events: none; }
  .ow-zone { width: 0; height: 0; }
  .ow-hedge { left: 0; right: 0; bottom: 0; height: 48px; z-index: -1; background: var(--ow-bush) 0 100% / 64px 48px repeat-x; image-rendering: pixelated; }
  .ow-hedge[data-z='dun'] { display: none; }
  .ow-hud { position: fixed; z-index: 25; top: 0; left: 0; right: 0; height: var(--hud-h, 40px); display: flex; align-items: center; gap: 28px; padding: 0 18px; pointer-events: none; background: #0f0b08; border-bottom: 4px solid #5b4322; font-family: 'Press Start 2P', ui-monospace, monospace; font-size: 9px; color: #f8ecd0; }
  .ow-hearts { display: flex; align-items: center; gap: 5px; }
  .ow-k { margin-right: 8px; color: #ff8a78; text-transform: uppercase; }
  .ow-hearts svg { overflow: visible; }
  .ow-hearts .e { fill: #3a1b1b; stroke: #6a2c2c; stroke-width: .35; }
  .ow-hearts .f { fill: #ff4a4a; }
  .ow-gems { display: flex; align-items: center; gap: 7px; }
  .ow-gems b { font-weight: 400; color: #7bf0a6; font-size: 11px; }
  .ow-x { color: #c9b78f; }
  .ow-map { margin-left: 4px; padding: 3px 6px; border: 3px solid #5b4322; background: #17110c; }
  .ow-map .room { fill: #17110c; stroke: #c9b78f; stroke-width: 1.5; }
  .ow-map .room.on { fill: #ffd54f; stroke: #fff2b8; }
  .ow-sage { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; justify-content: flex-end; }
  .ow-sage p { margin: 0; min-width: 0; padding: 6px 10px; border: 3px solid #5b4322; background: #efdfb0; color: #2e2010; font-size: 8px; line-height: 1.3; white-space: nowrap; overflow: hidden; text-overflow: clip; position: relative; }
  .ow-sage p::before { content: ''; position: absolute; left: -9px; top: 50%; translate: 0 -50%; border: 5px solid transparent; border-right-color: #5b4322; border-left: 0; }
  .ow-sage p span { display: inline-block; max-width: 100%; vertical-align: top; overflow: hidden; }
  @media (prefers-reduced-motion: no-preference) {
    .ow-hearts svg { animation: ow-beat 1.6s steps(1, end) infinite; transform-origin: 50% 60%; }
    .ow-hearts svg:nth-of-type(2) { animation-delay: .1s; }
    .ow-hearts svg:nth-of-type(3) { animation-delay: .2s; }
    .ow-gems.pop svg { animation: ow-hop .3s steps(3, end); }
    .ow-map .room.on { animation: ow-flash 1s steps(1, end) infinite; }
    .ow-sage p span { animation: ow-type 1.4s steps(36, end) backwards; }
    .ow-sage svg { animation: ow-sway 2.4s steps(2, jump-none) infinite; }
  }
  @keyframes ow-beat { 0%, 100% { scale: 1; } 8% { scale: 1.18; } 16% { scale: 1; } }
  @keyframes ow-hop { 50% { translate: 0 -6px; } }
  @keyframes ow-flash { 50% { fill: #fff2b8; } }
  @keyframes ow-type { from { max-width: 0; } }
  @keyframes ow-sway { 50% { translate: 0 -2px; } }
  @media (max-width: 1100px) { .ow-sage p { display: none; } }
  @media (max-width: 760px) { .ow-map, .ow-k { display: none; } .ow-hud { gap: 14px; } }
</style>
