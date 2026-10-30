<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // The caption that blinks under the wipe line after each page change.
  const CAPS: [string, string][] = [
    ['/library', 'READING LIBRARY'],
    ['/machines', 'READING MACHINES'],
    ['/history', 'READING HISTORY'],
    ['/settings', 'READING SETTINGS'],
    ['/', 'READING HOME'],
  ];
  const cap = $derived((CAPS.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p))) ?? CAPS[4])[1]);
</script>

{#if !stream}
  <div class="am-bg" aria-hidden="true"><i class="scan"></i></div>
  {#key path}
    <div class="am-wipe" aria-hidden="true"><b>{cap}</b></div>
  {/key}
{/if}

<style>
  .am-bg { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: #cdc2a3; }
  .am-bg::before { content: ''; position: absolute; inset: 0; opacity: 0.5; background: radial-gradient(circle, #8f8466 0.9px, transparent 1.2px) 0 0 / 22px 19px, radial-gradient(circle, #8f8466 0.9px, transparent 1.2px) 11px 9.5px / 22px 19px; }
  .am-bg::after { content: ''; position: absolute; inset: 0; background: radial-gradient(ellipse at 50% 40%, transparent 55%, rgb(70 55 30 / 0.22)); }
  .scan { position: absolute; left: 0; right: 0; top: 0; height: 1px; background: #5a4a30; opacity: 0.35; box-shadow: 0 0 14px 5px rgb(90 74 48 / 0.12); }
  .am-wipe { position: fixed; z-index: 55; left: 233px; right: 0; top: 0; height: 100vh; pointer-events: none; opacity: 0; }
  .am-wipe b { position: absolute; left: 40px; top: 50%; translate: 0 -50%; padding: 4px 10px; font: 400 13px/1 'Share Tech Mono', ui-monospace, monospace; letter-spacing: 0.2em; color: #e8dcc0; background: #2a2218; }
  @media (prefers-reduced-motion: no-preference) {
    .scan { animation: am-scan 14s linear infinite; }
    .am-wipe { animation: am-wipe 0.7s steps(1, end) both; }
    .am-wipe::before { content: ''; position: absolute; left: 0; right: 0; top: 0; height: 100%; background: #cdc2a3; border-bottom: 1px solid #2a2218; animation: am-wipe-bar 0.7s cubic-bezier(0.6, 0, 0.3, 1) both; }
    .am-wipe b { animation: am-blink 0.7s steps(1, end) both; }
  }
  @media (prefers-reduced-motion: reduce) { .am-wipe { display: none; } }
  @keyframes am-scan { from { translate: 0 -2vh; } to { translate: 0 102vh; } }
  @keyframes am-wipe { 0%, 99% { opacity: 1; } 100% { opacity: 0; } }
  @keyframes am-wipe-bar { 0% { clip-path: inset(0 0 100% 0); } 35% { clip-path: inset(0); } 100% { clip-path: inset(100% 0 0 0); } }
  @keyframes am-blink { 0%, 30% { opacity: 0; } 40%, 55% { opacity: 1; } 62%, 72% { opacity: 0; } 80%, 100% { opacity: 1; } }
</style>
