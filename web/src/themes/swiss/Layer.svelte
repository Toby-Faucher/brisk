<script lang="ts">
  // Swiss: a 12-column hairline grid behind the page, and a page name on <html> for the oversized kickers in theme.css.
  let { path }: { path: string } = $props();
  const name = $derived(path === '/' ? 'home' : (path.split('/')[1] ?? 'home'));
  $effect(() => {
    const root = document.documentElement;
    root.dataset.swissPage = name;
    return () => delete root.dataset.swissPage;
  });
</script>

{#if name !== 'stream'}
  <div class="grid" aria-hidden="true"></div>
{/if}

<style>
  /* behind the content (negative z-index, above the canvas), never over it; no pointer events */
  .grid {
    position: fixed;
    top: 0;
    bottom: 0;
    left: calc(var(--sw) + var(--pad));
    right: var(--pad);
    z-index: -1;
    pointer-events: none;
    background-image: linear-gradient(to right, var(--grid) 1px, transparent 1px);
    background-size: calc(100% / 12) 100%;
    border-right: 1px solid var(--grid);
  }
</style>
