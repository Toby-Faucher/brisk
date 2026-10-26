<script lang="ts">
  import { travel } from '../../ui/motion';
  import Icon from '../../ui/Icon.svelte';

  /** bawn -> this laptop. Decoration only (aria-hidden): the Network row next to it says the same in words. */
  let { path }: { path: 'direct' | 'relay' | null } = $props();

  let pkt: HTMLElement;
  $effect(() => {
    if (!path) return;
    return travel(pkt, path === 'relay' ? 3200 : 1600) ?? undefined; // one compositor loop; cancelled on destroy
  });
</script>

<div class="cp {path ?? 'none'}" aria-hidden="true">
  <span class="node"><Icon name="host" size={13} /></span>
  <span class="lane">
    <i class="line"></i>
    {#if path === 'relay'}<i class="rnode"></i>{/if}
    <span class="pkt" bind:this={pkt}><i></i></span>
  </span>
  <span class="node"><svg viewBox="0 0 24 24" width="13" height="13" fill="currentColor"><rect x="4" y="5" width="16" height="11" rx="2" /><rect x="2" y="17.5" width="20" height="2" rx="1" /></svg></span>
</div>

<style>
  .cp { display: flex; align-items: center; gap: 8px; height: 22px; margin: -6px 0 14px; color: var(--muted); --c: var(--accent); }
  .cp.relay { --c: var(--warn); }
  .cp.none { --c: var(--dim); opacity: .6; }
  .node { flex: 0 0 auto; width: 22px; height: 22px; border-radius: 50%; display: grid; place-items: center; background: var(--panel2); border: 1px solid var(--line); }
  .lane { position: relative; flex: 1; height: 22px; overflow: hidden; }
  .line { position: absolute; left: 0; right: 0; top: 50%; border-top: 2px solid color-mix(in srgb, var(--c) 45%, transparent); transform: translateY(-1px); }
  .relay .line, .none .line { border-top-style: dashed; }
  .rnode { position: absolute; left: 50%; top: 50%; width: 10px; height: 10px; margin: -5px 0 0 -5px; border-radius: 50%; background: var(--panel2); border: 2px solid var(--c); }
  /* resting (reduced motion, no path, tab hidden before it starts): the dot is simply not there */
  .pkt { position: absolute; inset: 0; opacity: 0; }
  .pkt i { position: absolute; right: 0; top: 50%; width: 6px; height: 6px; margin-top: -3px; border-radius: 50%; background: var(--c); }
</style>
