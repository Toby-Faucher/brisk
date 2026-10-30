<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // The readout that flashes in after each page change: a code and a name of our own.
  const CARDS: [string, string, string][] = [
    ['/library', 'LIBRARY', '0x1B'],
    ['/machines', 'MACHINES', '0x2C'],
    ['/history', 'HISTORY', '0x3D'],
    ['/settings', 'SETTINGS', '0x4E'],
    ['/', 'HOME', '0x0A'],
  ];
  const card = $derived(CARDS.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p))) ?? CARDS[4]);
</script>

{#if !stream}
  <div class="cy-bg" aria-hidden="true">
    <i class="grid"></i>
    <i class="scan"></i>
    <i class="br tl"></i>
    <i class="br tr"></i>
    <i class="br bl"></i>
    <i class="br brr"></i>
    <i class="warn"></i>
    <i class="bar"></i>
    <b class="read">SYS.LINK // {card[2]} // ONLINE</b>
  </div>
  <div class="cy-card" aria-hidden="true">
    <small>ACCESS {card[2]}</small>
    <b>{card[1]}</b>
    <i></i>
  </div>
{/if}

<style>
  .cy-bg { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: #08080a; }
  .cy-bg i, .cy-bg b { position: absolute; display: block; }
  .grid { inset: 0; opacity: 0.5; background: linear-gradient(rgb(252 238 9 / 0.05) 1px, transparent 1px) 0 0 / 48px 48px, linear-gradient(90deg, rgb(252 238 9 / 0.05) 1px, transparent 1px) 0 0 / 48px 48px; mask-image: radial-gradient(ellipse at 70% 30%, #000, transparent 75%); }
  .scan { inset: 0; background: repeating-linear-gradient(0deg, rgb(255 255 255 / 0.025) 0 1px, transparent 1px 3px); }
  .br { width: 34px; height: 34px; border: 0 solid rgb(252 238 9 / 0.55); }
  .tl { left: 10px; top: 10px; border-width: 2px 0 0 2px; }
  .tr { right: 10px; top: 10px; border-width: 2px 2px 0 0; }
  .bl { left: 10px; bottom: 10px; border-width: 0 0 2px 2px; }
  .brr { right: 10px; bottom: 10px; border-width: 0 2px 2px 0; }
  .warn { right: 60px; top: 0; width: 220px; height: 10px; background: repeating-linear-gradient(-45deg, #fcee09 0 8px, #0a0a0a 8px 16px); opacity: 0.55; }
  .bar { right: 60px; bottom: 16px; width: 130px; height: 22px; opacity: 0.4; background: repeating-linear-gradient(90deg, #fcee09 0 2px, transparent 2px 4px, #fcee09 4px 5px, transparent 5px 9px, #fcee09 9px 12px, transparent 12px 14px); }
  .read { right: 200px; bottom: 18px; font: 400 11px/1 'Share Tech Mono', monospace; letter-spacing: 0.14em; color: rgb(0 240 255 / 0.7); }

  .cy-card { position: fixed; z-index: 55; left: 50%; top: 42%; pointer-events: none; display: grid; gap: 2px; min-width: 300px; padding: 14px 44px 16px 24px; color: #000; background: #fcee09; opacity: 0; translate: -50% -50%; clip-path: polygon(0 0, calc(100% - 22px) 0, 100% 22px, 100% 100%, 22px 100%, 0 calc(100% - 22px)); }
  .cy-card small { font: 400 12px/1 'Share Tech Mono', monospace; letter-spacing: 0.2em; }
  .cy-card b { font: 700 clamp(44px, 6vw, 78px)/1 Teko, 'Chakra Petch', sans-serif; letter-spacing: 0.08em; text-shadow: 3px 0 #00f0ff, -3px 0 #ff2a4d; }
  .cy-card i { height: 8px; margin-top: 4px; background: repeating-linear-gradient(-45deg, #000 0 6px, transparent 6px 12px); }
  @media (prefers-reduced-motion: no-preference) {
    .cy-card { animation: cy-card 1.1s 0.12s both; }
    .scan { animation: cy-roll 7s linear infinite; }
  }
  @keyframes cy-card {
    0% { opacity: 1; clip-path: inset(48% 0 48% 0); translate: -46% -50%; }
    14% { opacity: 1; clip-path: inset(0); translate: -53% -50%; }
    20% { translate: -50% -50%; }
    26% { translate: -48% -50%; filter: hue-rotate(90deg); }
    32% { translate: -50% -50%; filter: none; }
    74% { opacity: 1; translate: -50% -50%; }
    100% { opacity: 0; translate: -50% -50%; clip-path: inset(50% 0 50% 0); }
  }
  @keyframes cy-roll { to { translate: 0 3px; } }
  @media (prefers-reduced-motion: reduce) { .cy-card { display: none; } }
</style>
