<script lang="ts" module>
  import type { Box } from '../ui/motion-base';
  let last: Box | null = null; // the previous page's active item: module state survives the Shell remount
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { box, glide, onStreamRoute } from '../ui/motion-base';
  import { loadModules } from './registry';
  import HostPill from './HostPill.svelte';
  import Icon from '../ui/Icon.svelte';

  let { path }: { path: string } = $props();
  const mods = loadModules();
  const live = mods.filter((m) => !m.soon);
  const soon = mods.filter((m) => m.soon);
  const here = (p: string) => (p === '/' ? path === '/' : path === p || path.startsWith(p + '/'));

  // After a client navigation the highlight slides over from where it was (never on first load, never onto /stream).
  let nav: HTMLElement;
  onMount(() => {
    const el = nav.querySelector('[aria-current="page"]');
    const now = el && box(el);
    if (el && now && last && !onStreamRoute()) glide(el, last, now, { pseudo: '::before' });
    last = now;
  });
</script>

<aside class="side">
  <div class="brand"><i><Icon name="bolt" size={14} /></i> Brisk</div>
  <nav aria-label="Main" bind:this={nav}>
    {#each live as m (m.id)}
      {@const r = m.routes[0]}
      <a class="nav" href={r.path} aria-current={here(r.path) ? 'page' : undefined}><Icon name={m.icon} color={m.color} />{m.title}</a>
    {/each}
  </nav>
  {#if soon.length}
    <div class="sec">Coming</div>
    {#each soon as m (m.id)}
      <div class="nav soon" aria-disabled="true"><Icon name={m.icon} color={m.color} />{m.title}<span class="tag">soon</span></div>
    {/each}
  {/if}
  <HostPill />
</aside>

<style>
  .side { width: 218px; flex: 0 0 auto; height: 100vh; position: sticky; top: 0; background: color-mix(in srgb, var(--panel) 82%, transparent); backdrop-filter: blur(10px); border-right: 1px solid var(--line); padding: 14px 10px; display: flex; flex-direction: column; gap: 2px; }
  nav { display: contents; }
  .brand { display: flex; align-items: center; gap: 9px; padding: 4px 8px 14px; font-weight: 700; }
  .brand i { width: 22px; height: 22px; border-radius: 8px; background: var(--accent); display: grid; place-items: center; color: var(--accent-fg); }
  .nav { display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 10px; color: var(--muted); transition: background .18s, color .18s, transform .18s var(--ease); }
  a.nav:hover { background: var(--panel2); color: var(--fg); transform: translateX(2px); }
  .nav[aria-current='page'] { color: var(--fg); position: relative; }
  /* the highlight is a pseudo-element so the server render already has it; motion-base glide() moves it under the other
     items (z-index -1 in .side's stacking context, which backdrop-filter creates) */
  .nav[aria-current='page']::before { content: ''; position: absolute; inset: 0; z-index: -1; border-radius: inherit; background: var(--panel2); box-shadow: 0 0 0 1px var(--line) inset; transform-origin: 0 0; }
  .soon { opacity: .55; cursor: default; }
  .soon .tag { margin-left: auto; }
  .sec { font-size: 11px; font-weight: 600; letter-spacing: .08em; text-transform: uppercase; color: var(--dim); padding: 14px 10px 4px; }
</style>
