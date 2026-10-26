<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { enterWhenHydrated } from './reveal';
  import { settings } from '../state/settings';
  import { denied } from '../state/api';
  import { bindAccent } from '../modules/settings/accent';
  import Sidebar from './Sidebar.svelte';
  import Toasts from './Toasts.svelte';
  import CommandMenu from './CommandMenu.svelte';
  import QuitDialog from './QuitDialog.svelte';

  let { path, children }: { path: string; children?: Snippet } = $props();
  $effect(() => bindAccent(settings, document.documentElement));
  onMount(enterWhenHydrated);
</script>

<div class="app">
  <Sidebar {path} />
  <main class:page-in={!/^\/stream/.test(path)}>
    {#if $denied}
      <section class="denied" role="alert">
        <h1>bawn doesn't know this browser</h1>
        <p>
          Open this page from the link that includes <code>?s=…</code> (on bawn:
          <code>scripts/brisk-service.sh link</code>), or ask hostd to allow your tailnet login
          (<code>~/.config/brisk/allowed-users</code>).
        </p>
      </section>
    {/if}
    <div class="page" class:gone={$denied}>{@render children?.()}</div>
  </main>
</div>
<Toasts />
<CommandMenu />
<QuitDialog />

<style>
  .app { display: flex; min-height: 100vh; }
  main { flex: 1; min-width: 0; padding: 18px 20px; }
  .page { display: contents; }
  .gone { display: none; }
  .denied { max-width: 560px; margin: 12vh auto 0; padding: 22px 24px; border: 1px solid var(--line); border-radius: var(--r-card); background: var(--panel); }
  .denied h1 { margin: 0 0 8px; font-size: 1.15rem; }
  .denied p { margin: 0; color: var(--muted); line-height: 1.5; }
  .denied code { font-family: var(--mono); color: var(--fg); }
</style>
