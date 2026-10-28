<script lang="ts">
  import Icon from './Icon.svelte';

  /** The inside of a Quit button: power icon and/or label; while `busy` they hide (keeping their space) behind a small spinner, so the button never changes size. The button carries aria-busy and its own label. */
  let { busy, label = '', icon }: { busy: boolean; label?: string; icon?: number } = $props();
</script>

<span class="ql">
  <span class="in" class:off={busy}>{#if icon}<Icon name="power" size={icon} />{/if}{label}</span>
  {#if busy}<i class="sp" data-testid="quit-spinner" aria-hidden="true"></i>{/if}
</span>

<style>
  .ql { display: inline-grid; place-items: center; vertical-align: middle; }
  .ql > * { grid-area: 1 / 1; }
  .in { display: inline-flex; align-items: center; gap: 6px; }
  .in.off { visibility: hidden; }
  .sp { width: 12px; height: 12px; box-sizing: border-box; border-radius: 50%; border: 2px solid currentColor; border-top-color: transparent; animation: ql-spin .8s linear infinite; }
  /* reduced motion: a still ring with a gap, no turning */
  @media (prefers-reduced-motion: reduce) { .sp { animation: none; } }
  @keyframes ql-spin { to { rotate: 360deg; } }
</style>
