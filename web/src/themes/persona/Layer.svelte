<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // The calling card that flies across after each page change: a title and one plain line of our own.
  const CARDS: [string, string, string][] = [
    ['/library', 'LIBRARY', 'Pick one. Play it. Brag later.'],
    ['/machines', 'MACHINES', 'Check the engine room.'],
    ['/history', 'HISTORY', 'Every round, on the record.'],
    ['/settings', 'SETTINGS', 'Tune it until it purrs.'],
    ['/', 'HOME', 'Your stuff, dealt face up.'],
  ];
  const card = $derived(CARDS.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p))) ?? CARDS[4]);
</script>

{#if !stream}
  <div class="pr-bg" aria-hidden="true">
    <i class="shard s1"></i>
    <i class="shard s2"></i>
    <i class="shard s3"></i>
    <i class="dots"></i>
    <svg class="star" viewBox="0 0 100 100"><polygon points="50,2 60,36 97,33 68,56 80,92 50,71 19,93 31,57 3,36 40,35" /></svg>
  </div>
  <div class="pr-card" aria-hidden="true">
    <b>{card[1]}</b>
    <span>{card[2]}</span>
    <svg viewBox="0 0 100 100"><polygon points="50,2 60,36 97,33 68,56 80,92 50,71 19,93 31,57 3,36 40,35" /></svg>
  </div>
{/if}

<style>
  .pr-bg { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: #0b0b0b; }
  .pr-bg i, .pr-bg svg { position: absolute; display: block; }
  .shard { background: #5a000b; }
  .s1 { right: 0; top: 0; width: 46vw; height: 62vh; clip-path: polygon(52% 0, 100% 0, 100% 100%, 86% 58%, 70% 70%, 62% 34%, 40% 30%); opacity: .8; }
  .s2 { left: 12vw; bottom: 0; width: 60vw; height: 36vh; background: #2a0006; clip-path: polygon(0 100%, 8% 40%, 20% 62%, 31% 6%, 44% 52%, 58% 22%, 70% 70%, 100% 100%); }
  .s3 { right: 6vw; bottom: 0; width: 22vw; height: 50vh; background: #e60012; opacity: .22; clip-path: polygon(40% 0, 100% 30%, 70% 100%, 0 100%, 20% 50%); }
  .dots { inset: 0; opacity: .5; background: radial-gradient(circle, #e60012 1.4px, transparent 1.6px) 0 0 / 14px 14px; mask-image: linear-gradient(200deg, #000, transparent 38%); }
  .star { right: -4vw; bottom: -8vh; width: 38vmax; height: 38vmax; fill: none; stroke: #fff; stroke-width: .5; opacity: .08; rotate: 14deg; }

  .pr-card { position: fixed; z-index: 55; left: 50%; top: 44%; pointer-events: none; display: grid; gap: 4px; padding: 18px 54px 18px 30px; color: #000; background: #fff; transform-origin: 20% 80%; clip-path: polygon(0 6%, 100% 0, 96% 94%, 2% 100%); opacity: 0; translate: -50% -50%; rotate: -4deg; }
  .pr-card b { font: 400 clamp(48px, 7vw, 92px)/1 Anton, Impact, sans-serif; letter-spacing: .02em; font-style: italic; }
  .pr-card span { font: 800 italic 16px/1.2 Rubik, system-ui, sans-serif; background: #000; color: #fff; padding: 3px 12px; width: fit-content; rotate: 1.2deg; }
  .pr-card svg { position: absolute; right: 8px; top: 8px; width: 38px; fill: #e60012; rotate: 12deg; }
  @media (prefers-reduced-motion: no-preference) {
    .pr-card { animation: pr-card 1.5s .25s both; }
    .shard { animation: pr-sway 9s ease-in-out infinite alternate; }
    .s2 { animation-duration: 13s; }
  }
  @keyframes pr-card {
    0% { opacity: 1; translate: 90vw -50%; rotate: 18deg; scale: .7; }
    22% { opacity: 1; translate: -50% -50%; rotate: -5deg; scale: 1.08; }
    30% { translate: -50% -50%; rotate: -4deg; scale: 1; }
    72% { opacity: 1; translate: -50% -50%; rotate: -4deg; scale: 1; }
    100% { opacity: 1; translate: -170vw -90%; rotate: -22deg; scale: .9; }
  }
  @keyframes pr-sway { to { translate: 14px -10px; } }
  @media (prefers-reduced-motion: reduce) { .pr-card { display: none; } }
</style>
