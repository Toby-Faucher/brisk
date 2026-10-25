<script lang="ts">
  import type { Snippet } from 'svelte';
  import { ACCENT_HEX, settings } from '../state/settings';
  import Smoke from './Smoke.svelte';

  let { cols = 2, rows = 1, glow = true, class: cls = '', children }: { cols?: number; rows?: number; glow?: boolean; class?: string; children?: Snippet } = $props();
  let hot = $state(false);
  let woke = $state(false); // the smoke (and its WebGL context) is created on the first hover only
  const on = () => (hot = woke = true);
  const off = () => (hot = false);
</script>

<div class="card {cls}" class:hot style:grid-column="span {cols}" style:grid-row="span {rows}" onpointerenter={on} onpointerleave={off} onfocusin={on} onfocusout={off} role="group">
  {#if glow && woke}<div class="glow"><Smoke accent={ACCENT_HEX[$settings.accent]} active={hot} /></div>{/if}
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
  /* hover smoke under a scrim that keeps the text readable (round1b mockup) */
  .glow { position: absolute; inset: 0; z-index: -2; opacity: 0; transition: opacity .45s var(--ease); pointer-events: none; }
  .hot .glow { opacity: .9; }
  .card::after { content: ''; position: absolute; inset: 0; z-index: -1; pointer-events: none; background: linear-gradient(180deg, rgba(11, 12, 15, .05), rgba(11, 12, 15, .55)); transition: opacity .45s var(--ease); }
  .hot::after { opacity: .55; }
</style>
