<script lang="ts">
  import { onMount } from 'svelte';
  import { ringPulse } from './motion-base';

  /** The host status dot (host pill, Machines). When the state changes it sends out one ring; first paint never does. */
  let { state }: { state: 'ready' | 'busy' | 'down' | 'off' } = $props();

  let halo: HTMLElement;
  let seen: string | null = null; // 'off' before the first real state is "still loading", not an outage
  let anim: Animation | null = null;
  $effect(() => {
    const s = state;
    if (s === 'off' && seen === null) return;
    if (seen !== null && seen !== s) anim = ringPulse(halo);
    seen = s;
  });
  onMount(() => () => anim?.cancel());
</script>

<span class="dotw {state}"><span class="dot {state}"></span><span class="halo" aria-hidden="true" bind:this={halo}></span></span>

<style>
  .dotw { position: relative; display: inline-block; flex: 0 0 auto; width: 8px; height: 8px; --ring: var(--ok); }
  .dotw.busy { --ring: var(--warn); }
  .dotw.down, .dotw.off { --ring: var(--bad); }
  .dotw > :global(.dot) { display: block; }
  .halo { position: absolute; inset: 0; border-radius: 50%; background: var(--ring); opacity: 0; pointer-events: none; }
</style>
