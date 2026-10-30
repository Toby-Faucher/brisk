<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // A line that fades in and out after each page change.
  const LINES: Record<string, string> = {
    '/': 'Rest awhile.',
    '/library': 'The shelves are quiet.',
    '/machines': 'The forge still holds heat.',
    '/history': 'Old roads, long since walked.',
    '/settings': 'Mend what has worn thin.',
    '/launch': 'Steel yourself.',
  };
  const FALLBACK = ['Embers remember.', 'Not yet dark.', 'The fog is thick tonight.'];
  let line = $state('');
  let tick = $state(0);
  let seen = false;
  $effect(() => {
    const p = path;
    if (/^\/stream/.test(p)) return;
    const base = '/' + (p.split('/')[1] ?? '');
    line = LINES[base] ?? FALLBACK[tick % FALLBACK.length];
    tick++;
    seen = true;
  });

  // Fixed embers: a plain list, so the markup is the same on every render.
  const embers = Array.from({ length: 22 }, (_, i) => ({
    x: (i * 37 + 11) % 100,
    d: 9 + ((i * 7) % 11),
    s: -((i * 13) % 17),
    z: 2 + (i % 3),
    w: 20 + ((i * 29) % 60),
  }));
</script>

{#if !stream}
  <div class="bf" aria-hidden="true">
    <i class="fog f1"></i>
    <i class="fog f2"></i>
    <i class="vig"></i>
    {#each embers as e}
      <i class="em" style="left:{e.x}%;--d:{e.d}s;--s:{e.s}s;--z:{e.z}px;--w:{e.w}px"></i>
    {/each}
    {#key path}
      <div class="bars"><b class="hp"></b><b class="sp"></b></div>
    {/key}
  </div>
  {#key path}
    <div class="ban" data-seen={seen} aria-hidden="true"><span>{line}</span></div>
  {/key}
{/if}

<style>
  .bf { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; }
  .bf i, .bf div { position: absolute; display: block; }
  .vig { inset: 0; background: radial-gradient(120% 100% at 50% 40%, transparent 45%, rgb(0 0 0 / .65) 100%); }
  .fog { left: -20%; width: 140%; height: 45%; bottom: -8%; filter: blur(30px); opacity: .5; }
  .f1 { background: radial-gradient(closest-side, rgb(120 105 90 / .16), transparent); animation: bf-fog 60s ease-in-out infinite alternate; }
  .f2 { height: 35%; bottom: 8%; opacity: .35; background: radial-gradient(closest-side, rgb(150 120 90 / .14), transparent); animation: bf-fog 80s ease-in-out -30s infinite alternate-reverse; }
  .em { bottom: -10px; width: var(--z); height: var(--z); border-radius: 50%; background: #ff9a4a; box-shadow: 0 0 6px 1px #e8742a; opacity: 0; animation: bf-up var(--d) linear var(--s) infinite; }
  .bars { left: 214px; top: 10px; width: 160px; display: flex; flex-direction: column; gap: 3px; opacity: .85; }
  .bars b { display: block; height: 3px; background: #0a0908; border: 1px solid #2c2720; position: relative; }
  .bars b::after { content: ''; position: absolute; inset: 0; transform-origin: 0 50%; }
  .hp::after { background: #a63a2a; }
  .sp { width: 120px; }
  .sp::after { background: #8d9a62; animation: bf-stam 3.5s ease-out both; }
  .ban { position: fixed; z-index: 55; left: 0; right: 0; bottom: 14%; text-align: center; opacity: 0; animation: bf-ban 4.8s ease-in-out both; }
  .ban[data-seen='false'] { animation-delay: .6s; }
  .ban span { font: italic 400 30px/1.2 'IM Fell English', Georgia, serif; letter-spacing: .12em; color: #d8cdb6; text-shadow: 0 0 22px rgb(232 116 42 / .55), 0 2px 8px #000; }
  @keyframes bf-fog { to { transform: translateX(12%) scaleY(1.2); } }
  @keyframes bf-up {
    0% { transform: translate(0, 0); opacity: 0; }
    10% { opacity: .9; }
    50% { transform: translate(calc(var(--w) * .5px), -50vh); }
    100% { transform: translate(calc(var(--w) * -.4px), -108vh); opacity: 0; }
  }
  @keyframes bf-stam { 0% { transform: scaleX(.15); } 100% { transform: scaleX(1); } }
  @keyframes bf-ban { 0% { opacity: 0; letter-spacing: .05em; } 25%, 65% { opacity: 1; } 100% { opacity: 0; letter-spacing: .2em; } }
  @media (max-width: 800px) { .bars { display: none; } .ban span { font-size: 22px; } }
  @media (prefers-reduced-motion: reduce) {
    .fog, .em, .sp::after { animation: none !important; }
    .em { display: none; }
    .ban { animation: none; opacity: 0; }
  }
</style>
