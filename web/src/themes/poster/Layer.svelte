<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));
</script>

<!-- film grain and a soft vignette; decoration only, never over the stream -->
{#if !stream}
  <div class="poster-grain" aria-hidden="true"></div>
  <div class="poster-vignette" aria-hidden="true"></div>
{/if}

<style>
  .poster-grain,
  .poster-vignette {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 25;
  }
  .poster-grain {
    inset: -60px;
    opacity: 0.085;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='200'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.85' numOctaves='2' stitchTiles='stitch'/%3E%3CfeColorMatrix values='0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 1.4 -.2'/%3E%3C/filter%3E%3Crect width='200' height='200' filter='url(%23n)'/%3E%3C/svg%3E");
  }
  .poster-vignette {
    background: radial-gradient(120% 90% at 50% 40%, transparent 55%, rgb(0 0 0 / 0.55));
  }
  @media (prefers-reduced-motion: no-preference) {
    .poster-grain {
      animation: jitter 1.1s steps(1) infinite;
    }
  }
  @keyframes jitter {
    0% { translate: 0 0; }
    20% { translate: -23px 11px; }
    40% { translate: 17px -29px; }
    60% { translate: -9px -17px; }
    80% { translate: 26px 21px; }
  }
</style>
