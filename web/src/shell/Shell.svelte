<script lang="ts">
  import type { Snippet } from 'svelte';
  import { settings } from '../state/settings';
  import { bindAccent } from '../modules/settings/accent';
  import Sidebar from './Sidebar.svelte';
  import Toasts from './Toasts.svelte';
  import CommandMenu from './CommandMenu.svelte';

  let { path, children }: { path: string; children?: Snippet } = $props();
  $effect(() => bindAccent(settings, document.documentElement));
</script>

<div class="app">
  <Sidebar {path} />
  <main>{@render children?.()}</main>
</div>
<Toasts />
<CommandMenu />

<style>
  .app { display: flex; min-height: 100vh; }
  main { flex: 1; min-width: 0; padding: 18px 20px; }
</style>
