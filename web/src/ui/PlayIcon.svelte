<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { swapIcon } from './motion';

  /** The play icon of a Play/Resume button: while `busy` (a start is in flight) its triangle swaps for a spinner. Never on /stream. */
  let { busy, size = 14 }: { busy: boolean; size?: number } = $props();

  let root: HTMLElement;
  let icon: ReturnType<typeof swapIcon> | undefined;
  onMount(() => {
    icon = swapIcon(root.querySelector('svg')!);
    return () => icon?.destroy();
  });
  $effect(() => icon?.to(busy)); // runs after onMount: the first value is the end state, later ones swap
</script>

<span class="mi" bind:this={root}><Icon name="play" {size} /></span>

<style>
  .mi { display: contents; }
</style>
