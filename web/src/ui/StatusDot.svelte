<script lang="ts">
  import { onMount } from 'svelte';
  import { dotFade, ringPulse } from './motion-base';

  /**
   * The host status dot (host pill, Machines). When the state changes the old colour cross-fades into the new one and
   * one ring goes out; first paint never does.
   */
  let { state }: { state: 'ready' | 'busy' | 'down' | 'off' } = $props();

  let halo: HTMLElement;
  let was: HTMLElement;
  let seen: string | null = null; // 'off' before the first real state is "still loading", not an outage
  let anims: (Animation | null)[] = [];
  $effect(() => {
    const s = state;
    if (s === 'off' && seen === null) return;
    if (seen !== null && seen !== s) {
      anims.forEach((a) => a?.cancel());
      was.dataset.s = seen; // only this effect writes it: Svelte never touches the copy's attributes
      anims = [dotFade(was), ringPulse(halo)];
    }
    seen = s;
  });
  onMount(() => () => anims.forEach((a) => a?.cancel()));
</script>

<span class="dotw {state}"><span class="dot {state}"></span><span class="halo" aria-hidden="true" bind:this={halo}></span><span class="was" aria-hidden="true" bind:this={was}></span></span>

<style>
  .dotw { position: relative; display: inline-block; flex: 0 0 auto; width: 8px; height: 8px; --ring: var(--ok); }
  .dotw.busy { --ring: var(--warn); }
  .dotw.down, .dotw.off { --ring: var(--bad); }
  .dotw > :global(.dot) { display: block; }
  .halo, .was { position: absolute; inset: 0; border-radius: 50%; opacity: 0; pointer-events: none; }
  .halo { background: var(--ring); }
  /* the previous state's dot (same look as .dot in base.css), laid over the new one while it fades out */
  .was { --was: var(--ok); background: var(--was); box-shadow: 0 0 0 3px color-mix(in srgb, var(--was) 25%, transparent); }
  .was[data-s='busy'] { --was: var(--warn); }
  .was:is([data-s='down'], [data-s='off']) { --was: var(--bad); }
</style>
