<script lang="ts">
  import type { Component } from 'svelte';
  import { settings } from '../state/settings';

  let { path }: { path: string } = $props();
  // A style may ship themes/<id>/Layer.svelte (overlay, extra nav, canvas). Only the active one is fetched.
  const layers = import.meta.glob<{ default: Component<{ path: string }> }>('./*/Layer.svelte');
  let Layer: Component<{ path: string }> | null = $state(null);
  $effect(() => {
    const load = layers[`./${$settings.ui}/Layer.svelte`];
    Layer = null;
    if (!load) return;
    let live = true;
    void load().then((m) => live && (Layer = m.default));
    return () => (live = false);
  });
</script>

{#if Layer}<Layer {path} />{/if}
