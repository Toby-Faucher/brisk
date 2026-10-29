<script lang="ts" module>
  /** Context a Home slot (modules/home/Slot.svelte) sets: the card then carries `data-card` and a drag handle. */
  export const SLOT = Symbol('card-slot');
  export type Slot = { id: string; label: string };
</script>

<script lang="ts">
  import { getContext, type Snippet } from 'svelte';

  let { cols = 2, rows = 1, class: cls = '', style = '', children }: { cols?: number; rows?: number; class?: string; style?: string; children?: Snippet } = $props();
  const slot = getContext<Slot | undefined>(SLOT);
</script>

<div class="card reveal {cls}" {style} style:grid-column="span {cols}" style:grid-row="span {rows}" role="group" data-card={slot?.id}>
  {#if slot}
    <!-- aria-pressed is flipped by modules/home/drag.ts while the card is picked up -->
    <button type="button" class="grip" data-grip aria-label="Move {slot.label} card" aria-describedby="grip-hint" aria-pressed="false">
      <svg viewBox="0 0 12 18" width="12" height="18" aria-hidden="true" fill="currentColor">{#each [3, 9, 15] as y}<circle cx="3" cy={y} r="1.6" /><circle cx="9" cy={y} r="1.6" />{/each}</svg>
    </button>
  {/if}
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
    /* scale + box-shadow: the Home drag lift (data-lifted) settling back on the snappy spring */
    transition: transform var(--spring-soft-dur) var(--spring-soft), scale var(--spring-snappy-dur) var(--spring-snappy), box-shadow .25s, border-color .25s;
  }
  .card:hover { transform: translateY(-3px); border-color: var(--line-hover); }
  /* link cards (module widgets) give a little under the press; the snappy spring brings them back */
  .card:has(:global(.fill:active)) { transform: translateY(-1px) scale(.99); transition-timing-function: var(--spring-snappy); transition-duration: var(--spring-snappy-dur); }
  .card :global(h4) { font-size: 12px; font-weight: 600; color: #c4cad6; display: flex; gap: 8px; align-items: center; }
  /* full-card link overlays (module widgets): the ring sits inside, since the card clips overflow */
  .card :global(.fill:focus-visible) { outline-offset: -2px; border-radius: inherit; }
  /* above the .fill link overlay; touch-action so a touch drag on it moves the card, not the page */
  .grip { position: absolute; top: 8px; right: 8px; z-index: 2; width: 28px; height: 28px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 8px; background: transparent; color: var(--muted); cursor: grab; touch-action: none; transition: color .2s, background-color .2s; }
  .grip:hover, .grip:focus-visible, .grip:global([aria-pressed='true']) { color: var(--fg); background: color-mix(in srgb, #fff 8%, transparent); }
  .grip:global([aria-pressed='true']) { cursor: grabbing; }
  @media (prefers-reduced-motion: reduce) {
    .card:hover, .card:has(:global(.fill:active)) { transform: none; }
    .card { transition: border-color .25s; }
  }
</style>
