<script lang="ts" module>
  // One caller number per page load of the app, kept in this browser. Storage can be blocked, so it falls back quietly.
  let caller = 0;
  function nextCaller(): number {
    if (caller) return caller;
    let n = 1;
    try {
      n = (Number(localStorage.getItem('bbs.calls')) || 0) + 1;
      localStorage.setItem('bbs.calls', String(n));
    } catch {}
    return (caller = 1336 + n);
  }
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const onStream = $derived(/^\/stream/.test(path));
  const who = String(nextCaller()).padStart(5, '0');
  const pad = (n: number) => String(n).padStart(2, '0');
  let now = $state(new Date());
  let left = $state(60); // "time left today", minutes: a board joke, counts down while the tab is open
  onMount(() => {
    const t = setInterval(() => {
      now = new Date();
      left = Math.max(0, left - 1);
    }, 60000);
    return () => clearInterval(t);
  });
  const clock = $derived(`${pad(now.getHours())}:${pad(now.getMinutes())}`);
</script>

{#if !onStream}
  <svg class="bb-defs" aria-hidden="true" focusable="false">
    <filter id="bbs-ansi" color-interpolation-filters="sRGB">
      <feComponentTransfer>
        <feFuncR type="discrete" tableValues="0 0.33 0.67 1" />
        <feFuncG type="discrete" tableValues="0 0.33 0.67 1" />
        <feFuncB type="discrete" tableValues="0 0.33 0.67 1" />
      </feComponentTransfer>
    </filter>
  </svg>
  <div class="bb-top" aria-hidden="true">
    <span class="bb-hs">ATDT 555-0142 ... CONNECT 14400/ARQ/V42BIS ... Welcome to Brisk BBS!</span>
    <span class="bb-r">Caller #{who} <i>|</i> Node 1 <i>|</i> {clock}</span>
  </div>
  <div class="bb-bot" aria-hidden="true">
    <span><b>/</b> Search</span>
    <span><b>Ctrl+K</b> Commands</span>
    <span><b>Alt+U</b> Style</span>
    <span class="bb-r">Time left: {left} min <span class="bb-cur">&#9608;</span></span>
  </div>
{/if}

<style>
  .bb-defs { position: absolute; width: 0; height: 0; }
  .bb-top, .bb-bot { position: fixed; left: 0; right: 0; z-index: 30; pointer-events: none; display: flex; align-items: center; gap: 22px; padding: 0 16px; font-family: 'IBM Plex Mono', ui-monospace, monospace; font-size: 13px; white-space: nowrap; overflow: hidden; color: #fff; background: #0000aa; }
  .bb-top { top: 0; height: 34px; border-bottom: 2px solid #55ffff; }
  .bb-bot { bottom: 0; height: 32px; border-top: 2px solid #55ffff; }
  .bb-hs { color: #55ff55; overflow: hidden; display: inline-block; max-width: 100%; }
  .bb-r { margin-left: auto; color: #ffff55; display: inline-flex; gap: 8px; }
  .bb-r i { font-style: normal; color: #55ffff; }
  .bb-bot b { color: #ffff55; margin-right: 4px; }
  .bb-cur { color: #55ffff; }
  @media (max-width: 900px) { .bb-hs { display: none; } .bb-bot span:not(.bb-r) { display: none; } }
  @media (prefers-reduced-motion: no-preference) {
    .bb-hs { animation: bb-type 3.2s steps(70, end) backwards; }
    .bb-cur { animation: bb-blink 1s steps(1) infinite; }
  }
  @keyframes bb-type { from { max-width: 0; } }
  @keyframes bb-blink { 50% { opacity: 0; } }
</style>
