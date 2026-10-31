<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { pageSwap } from '../ui/motion-base';
  import { enterWhenHydrated } from './reveal';
  import { settings } from '../state/settings';
  import { denied } from '../state/api';
  import { hub, selectedMachine, startMachines, powerMachine } from '../state/machines';
  import { toastError } from './Toasts.svelte';
  import WakeOverlay from '../ui/WakeOverlay.svelte';
  import { bindAccent } from '../modules/settings/accent';
  import { bindUi, nextUi } from '../themes/apply';
  import ThemeLayer from '../themes/ThemeLayer.svelte';
  import Sidebar from './Sidebar.svelte';
  import Toasts from './Toasts.svelte';
  import CommandMenu from './CommandMenu.svelte';
  import QuitDialog from './QuitDialog.svelte';
  import { attachPadNav } from './padnav';

  let { path, children }: { path: string; children?: Snippet } = $props();
  $effect(() => bindAccent(settings, document.documentElement));
  $effect(() => bindUi(settings, document.documentElement));
  // Alt+U / Alt+Shift+U flips through the UI styles without opening Settings.
  function cycleUi(e: KeyboardEvent) {
    if (!e.altKey || e.ctrlKey || e.metaKey || e.code !== 'KeyU') return;
    e.preventDefault();
    settings.update((s) => ({ ...s, ui: nextUi(s.ui, e.shiftKey ? -1 : 1) }));
  }
  // The pad drives the UI everywhere but /stream, where it belongs to the game.
  $effect(() => (/^\/stream/.test(path) ? undefined : attachPadNav()));
  onMount(() => (enterWhenHydrated(), startMachines())); // startMachines returns its cleanup
  // A changed machine (Use, host pill, Ctrl+K) dips the page while its data reloads; the first selection and a remount don't.
  let main: HTMLElement | undefined = $state();
  let seen: string | null = null;
  $effect(() => {
    const id = $selectedMachine?.id ?? null, was = seen;
    seen = id;
    if (was && id && was !== id && main) pageSwap(main);
  });
  // Opening the app with the machine off never starts it; the banner offers to.
  const off = $derived($hub === true && $selectedMachine?.state === 'off' ? $selectedMachine : null);
</script>

<svelte:window onkeydown={cycleUi} />
<div class="app">
  <Sidebar {path} />
  <main bind:this={main} class:page-in={!/^\/stream/.test(path)}>
    {#if $denied}
      <section class="denied" role="alert">
        <h1>This host doesn't know this browser</h1>
        <p>
          Open this page from the link that includes <code>?s=…</code> (on the host:
          <code>scripts/brisk-service.sh link</code>), or ask hostd to allow your tailnet login
          (<code>~/.config/brisk/allowed-users</code>).
        </p>
      </section>
    {/if}
    {#if off}
      <div class="off" role="status">
        <span>{off.name} is off</span>
        {#if off.canPower}<button class="btn sm" onclick={() => void powerMachine(off.id, 'start').catch((e) => toastError(`start ${off.name}`, e))}>Start</button>{/if}
      </div>
    {/if}
    <div class="page" class:gone={$denied}>{@render children?.()}</div>
  </main>
</div>
<Toasts />
<CommandMenu />
<QuitDialog />
<WakeOverlay />
<ThemeLayer {path} />

<style>
  .app { display: flex; min-height: 100vh; }
  main { flex: 1; min-width: 0; padding: 18px 20px; }
  .off { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 14px; padding: 8px 14px; border: 1px solid var(--line); border-radius: 12px; background: var(--panel); color: var(--muted); }
  .off .sm { padding: 4px 12px; font-size: 13px; }
  .page { display: contents; }
  .gone { display: none; }
  .denied { max-width: 560px; margin: 12vh auto 0; padding: 22px 24px; border: 1px solid var(--line); border-radius: var(--r-card); background: var(--panel); }
  .denied h1 { margin: 0 0 8px; font-size: 1.15rem; }
  .denied p { margin: 0; color: var(--muted); line-height: 1.5; }
  .denied code { font-family: var(--mono); color: var(--fg); }
</style>
