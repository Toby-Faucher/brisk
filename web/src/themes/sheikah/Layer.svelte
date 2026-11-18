<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // Plain rune-like marks (original, not from any script): each is a few strokes in a 20x20 box.
  const RUNES = [
    'M10 2v16M4 6h12M6 14l4-4 4 4',
    'M4 3l6 7-6 7M16 3l-6 7 6 7',
    'M10 2a8 8 0 100 16M10 2v16M10 10h8',
    'M3 17L10 3l7 14M6 12h8',
    'M4 4h12M10 4v14M4 18h12',
    'M10 2l8 8-8 8-8-8zM10 7v6',
    'M3 10h14M10 3v14M5 5l10 10',
  ];
</script>

{#if !stream}
  <div class="sk-bg" aria-hidden="true">
    <svg class="eye" viewBox="0 0 200 200">
      <circle cx="100" cy="100" r="92" />
      <circle cx="100" cy="100" r="70" stroke-dasharray="2 7" />
      <circle cx="100" cy="100" r="30" />
      <path d="M8 100h40M152 100h40M100 8v40M100 152v40" />
      <circle cx="100" cy="100" r="8" class="pupil" />
    </svg>
    <i class="ember"></i>
  </div>
  <div class="sk-scan" aria-hidden="true">
    <svg class="ring" viewBox="0 0 200 200"><circle cx="100" cy="100" r="86" pathLength="1" /></svg>
    <div class="runes">
      {#each RUNES as d, i (i)}
        <svg viewBox="0 0 20 20" style="--i:{i}"><path {d} pathLength="1" /></svg>
      {/each}
    </div>
    <i class="line"></i>
  </div>
{/if}

<style>
  .sk-bg { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: radial-gradient(ellipse at 70% 0, #14292b 0, #0a1315 60%), #0a1315; }
  .sk-bg::after { content: ''; position: absolute; inset: 0; opacity: .5; background: repeating-linear-gradient(0deg, rgb(255 255 255 / .018) 0 1px, transparent 1px 4px); }
  .eye { position: absolute; right: -14vmax; bottom: -14vmax; width: 52vmax; height: 52vmax; fill: none; stroke: #5cf0ff; stroke-width: .7; opacity: .1; }
  .pupil { fill: #5cf0ff; }
  .ember { position: absolute; left: -10vw; bottom: -20vh; width: 60vw; height: 50vh; background: radial-gradient(ellipse, rgb(255 163 71 / .09), transparent 65%); }

  .sk-scan { position: fixed; inset: 0; z-index: 55; pointer-events: none; display: grid; place-items: center; opacity: 0; }
  .ring { position: absolute; width: min(60vmin, 420px); fill: none; stroke: #5cf0ff; stroke-width: 1.5; stroke-linecap: round; filter: drop-shadow(0 0 6px #5cf0ff); rotate: -90deg; }
  .ring circle { stroke-dasharray: 1; stroke-dashoffset: 1; }
  .runes { display: flex; gap: clamp(8px, 2vw, 22px); }
  .runes svg { width: clamp(22px, 4vw, 40px); fill: none; stroke: #5cf0ff; stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; filter: drop-shadow(0 0 5px #5cf0ff); }
  .runes path { stroke-dasharray: 1; stroke-dashoffset: 1; }
  .line { position: absolute; top: 0; bottom: 0; left: 0; width: 2px; background: #5cf0ff; box-shadow: 0 0 24px 6px rgb(92 240 255 / .5); }

  @media (prefers-reduced-motion: no-preference) {
    .sk-scan { animation: sk-fade 1.25s ease-out both; }
    .ring circle { animation: sk-draw .8s .05s ease-out forwards; }
    .runes path { animation: sk-draw .5s calc(.1s + var(--i, 0) * 60ms) ease-out forwards; }
    .line { animation: sk-sweep .9s cubic-bezier(.6, 0, .3, 1) both; }
    .eye { animation: sk-spin 120s linear infinite; }
  }
  @keyframes sk-fade { 0% { opacity: 1; } 65% { opacity: 1; } 100% { opacity: 0; } }
  @keyframes sk-draw { to { stroke-dashoffset: 0; } }
  @keyframes sk-sweep { from { translate: 0 0; } to { translate: calc(100vw - 2px) 0; } }
  @keyframes sk-spin { to { rotate: 360deg; } }
  @media (prefers-reduced-motion: reduce) { .sk-scan { display: none; } }
</style>
