<script lang="ts">
  // Confetti behind the page: shapes sit in the gutters. Never on /stream; never catches a click.
  let { path }: { path: string } = $props();
  const on = $derived(!/^\/stream/.test(path));
</script>

{#if on}
  <div class="confetti" aria-hidden="true">
    <svg class="s a" viewBox="0 0 100 100" width="90" height="90"><path d="M50 6 61 38 95 38 67 58 78 92 50 71 22 92 33 58 5 38 39 38Z" fill="#ff5fa2" stroke="#141210" stroke-width="5" stroke-linejoin="round" /></svg>
    <svg class="s b" viewBox="0 0 120 30" width="120" height="30"><path d="M4 15q15-20 28 0t28 0 28 0 28 0" fill="none" stroke="#2f5bff" stroke-width="7" stroke-linecap="round" /></svg>
    <svg class="s c" viewBox="0 0 80 80" width="70" height="70"><circle cx="40" cy="40" r="34" fill="#35d07f" stroke="#141210" stroke-width="5" /></svg>
    <svg class="s d" viewBox="0 0 90 80" width="80" height="72"><path d="M45 8 84 72H6Z" fill="#4fd8ff" stroke="#141210" stroke-width="5" stroke-linejoin="round" /></svg>
    <svg class="s g" viewBox="0 0 120 30" width="110" height="28"><path d="M4 15q15-20 28 0t28 0 28 0 28 0" fill="none" stroke="#ff5fa2" stroke-width="7" stroke-linecap="round" /></svg>
    <svg class="s h" viewBox="0 0 80 80" width="54" height="54"><circle cx="40" cy="40" r="28" fill="none" stroke="#141210" stroke-width="8" /></svg>
  </div>
{/if}

<style>
  .confetti { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; }
  .s { position: absolute; }
  .a { right: -62px; bottom: 90px; rotate: 14deg; }
  .b { left: -90px; bottom: 150px; rotate: -20deg; }
  .c { left: -44px; top: 46%; }
  .d { right: -56px; top: 38%; rotate: 12deg; }
  .g { right: -100px; top: 190px; rotate: 70deg; }
  .h { left: -34px; top: 220px; }
  @media (prefers-reduced-motion: no-preference) {
    .s { animation: bob 5s ease-in-out infinite; }
    .b, .d, .h { animation-duration: 7s; animation-delay: -2s; }
    .c, .g { animation-duration: 6s; animation-delay: -4s; }
  }
  @keyframes bob { 50% { translate: 0 -14px; } }
  @media (max-width: 900px) { .confetti { display: none; } }
</style>
