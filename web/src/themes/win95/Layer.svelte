<script lang="ts">
  // Taskbar and Start menu for the Classic desktop style. Real links only; the host pill stays in the explorer pane.
  import { onMount } from 'svelte';
  import { loadModules } from '../../shell/registry';
  import { host } from '../../state/host';
  import { hub, selectedMachine } from '../../state/machines';
  import { STATE_LABEL } from '../../modules/machines/machines';
  import Icon from '../../ui/Icon.svelte';

  let { path }: { path: string } = $props();
  const mods = loadModules().filter((m) => !m.soon);
  const here = (p: string) => (p === '/' ? path === '/' : path === p || path.startsWith(p + '/'));
  const current = $derived(mods.find((m) => here(m.routes[0].path)));
  const SUN = { ready: 'Sunshine ready', busy: 'Streaming', down: 'Sunshine down' } as const;
  const hostLine = $derived(
    $hub === true
      ? `${$selectedMachine?.name ?? '-'}: ${$selectedMachine ? STATE_LABEL[$selectedMachine.state] : 'none selected'}`
      : `${$host?.name ?? '-'}: ${$host ? SUN[$host.sunshine] : 'not reachable'}`,
  );

  let open = $state(false);
  let root = $state<HTMLElement>();
  let now = $state(new Date());
  onMount(() => {
    const t = setInterval(() => (now = new Date()), 15_000);
    return () => clearInterval(t);
  });
  const time = $derived(now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }));
  const menuKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      open = false;
      root?.querySelector<HTMLElement>('.w95-start')?.focus();
    }
  };
  const runBox = () => {
    open = false;
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }));
  };
</script>

<svelte:window onkeydown={(e) => open && menuKey(e)} onpointerdown={(e) => open && !root?.contains(e.target as Node) && (open = false)} />

{#if !path.startsWith('/stream')}
<div class="w95-bar" bind:this={root}>
  {#if open}
    <div class="w95-menu" role="group" aria-label="Start menu">
      <div class="w95-ban" aria-hidden="true"><span>Brisk<b>95</b></span></div>
      <nav aria-label="Start menu">
        {#each mods as m (m.id)}
          <a class="w95-it" href={m.routes[0].path} aria-current={here(m.routes[0].path) ? 'page' : undefined} onclick={() => (open = false)}><Icon name={m.icon} color={m.color} />{m.title}</a>
        {/each}
        <hr />
        <button class="w95-it" onclick={runBox}>Run... <span class="w95-k">Ctrl+K</span></button>
        <hr />
        <div class="w95-host"><Icon name="host" color="var(--ok)" />{hostLine}</div>
      </nav>
    </div>
  {/if}
  <button class="w95-start" aria-haspopup="true" aria-expanded={open} onclick={() => (open = !open)}><span class="w95-flag" aria-hidden="true"></span>Start</button>
  <span class="w95-sep" aria-hidden="true"></span>
  {#if current}<a class="w95-task" href={current.routes[0].path} aria-current="page"><Icon name={current.icon} color={current.color} />{current.title}</a>{/if}
  <div class="w95-tray"><span class="w95-led" aria-hidden="true"></span>{hostLine}<time datetime={now.toISOString()}>{time}</time></div>
</div>
{/if}
