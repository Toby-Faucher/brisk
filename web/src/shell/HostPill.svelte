<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { host, pollHost } from '../state/host';
  import { hub, machines, selected, selectedMachine, selectMachine } from '../state/machines';
  import { dotState, STATE_LABEL } from '../modules/machines/machines';
  import { fadeOnChange } from '../ui/motion-base';
  import Icon from '../ui/Icon.svelte';
  import StatusDot from '../ui/StatusDot.svelte';

  onMount(() => pollHost()); // every 10 s while visible; cleaned up on each remount
  const label = { ready: 'Sunshine ready', busy: 'Streaming', down: 'Sunshine down' } as const;

  let open = $state(false);
  let root: HTMLElement;
  const sel = $derived($selectedMachine);
  const items = () => [...root.querySelectorAll<HTMLElement>('[role="menuitemradio"]')];

  async function toggle(to = !open) {
    open = to;
    if (to) (await tick(), (items().find((b) => b.getAttribute('aria-checked') === 'true') ?? items()[0])?.focus());
  }
  function key(e: KeyboardEvent) {
    const list = items();
    const at = list.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') (e.preventDefault(), (open = false), root.querySelector<HTMLElement>('.pill')?.focus());
    else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') (e.preventDefault(), list.at((at + (e.key === 'ArrowDown' ? 1 : -1)) % list.length)?.focus());
    else if (e.key === 'Home' || e.key === 'End') (e.preventDefault(), (e.key === 'Home' ? list[0] : list.at(-1))?.focus());
  }
  function choose(id: string) {
    open = false;
    root.querySelector<HTMLElement>('.pill')?.focus();
    void selectMachine(id);
  }
</script>

<svelte:window onpointerdown={(e) => open && !root.contains(e.target as Node) && (open = false)} />

{#if $hub === true}
  <div class="wrap" bind:this={root}>
    {#if open}
      <div class="menu glass" role="menu" aria-label="Machines" tabindex="-1" onkeydown={key}>
        {#each $machines as m (m.id)}
          <button class="it" role="menuitemradio" aria-checked={m.id === $selected} tabindex="-1" onclick={() => choose(m.id)}>
            <StatusDot state={dotState(m)} /><span class="name">{m.name}</span><span class="st">{STATE_LABEL[m.state]}</span>
          </button>
        {/each}
        <a class="it all" role="menuitem" tabindex="-1" href="/machines">All machines</a>
      </div>
    {/if}
    <button class="pill" aria-haspopup="menu" aria-expanded={open} onclick={() => void toggle()} title={sel ? STATE_LABEL[sel.state] : 'No machine selected'}>
      <Icon name="host" color={sel?.state === 'ready' ? 'var(--ok)' : 'var(--muted)'} />
      <span class="name" {@attach fadeOnChange(sel?.name ?? '–')}>{sel?.name ?? '–'}</span>
      {#if sel}<span class="st">{STATE_LABEL[sel.state]}</span>{/if}
      <StatusDot state={sel ? dotState(sel) : 'off'} />
    </button>
  </div>
{:else}
  <a class="pill" href="/machines" title={$host ? label[$host.sunshine] : 'Host not reachable'}>
    <Icon name="host" color={$host ? 'var(--ok)' : 'var(--muted)'} />
    <span class="name">{$host?.name ?? '–'}</span>
    <StatusDot state={$host?.sunshine ?? 'off'} />
  </a>
{/if}

<style>
  .pill { margin-top: auto; display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 10px; color: var(--muted); transition: background .18s, color .18s; }
  .pill:hover { background: var(--panel2); color: var(--fg); }
  .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .wrap { margin-top: auto; position: relative; display: grid; }
  button.pill { margin-top: 0; width: 100%; border: 0; background: none; font: inherit; cursor: pointer; text-align: left; }
  .st { font-size: 12px; color: var(--muted); }
  .menu { position: absolute; left: 0; right: 0; bottom: calc(100% + 6px); padding: 6px; display: grid; gap: 2px; z-index: 40; border-radius: 14px; }
  .it { display: flex; align-items: center; gap: 10px; padding: 7px 10px; border: 0; border-radius: 10px; background: none; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .it:hover, .it:focus-visible { background: var(--panel2); }
  .it[aria-checked='true'] .name { font-weight: 600; }
  .all { color: var(--muted); }
  @media (prefers-reduced-motion: no-preference) {
    .menu { animation: enter-rise var(--dur-enter) var(--ease) backwards; }
  }
</style>
