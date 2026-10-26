<script lang="ts">
  import type { Snippet } from 'svelte';

  let { cols = 2, rows = 1, class: cls = '', children }: { cols?: number; rows?: number; class?: string; children?: Snippet } = $props();
</script>

<div class="card reveal {cls}" style:grid-column="span {cols}" style:grid-row="span {rows}" role="group">
  {@render children?.()}
</div>

<style>
  .card {
    background: color-mix(in srgb, var(--panel) 78%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--r-card);
    padding: 16px;
    position: relative;
    overflow: hidden;
    isolation: isolate;
    min-width: 0;
    transition: transform .25s var(--ease), border-color .25s;
  }
  .card:hover { transform: translateY(-3px); border-color: #3a4152; }
  .card :global(h4) { font-size: 12px; font-weight: 600; color: #c4cad6; display: flex; gap: 8px; align-items: center; }
  /* full-card link overlays (module widgets): the ring sits inside, since the card clips overflow */
  .card :global(.fill:focus-visible) { outline-offset: -2px; border-radius: inherit; }
  @media (prefers-reduced-motion: reduce) { .card:hover { transform: none; } }
</style>
