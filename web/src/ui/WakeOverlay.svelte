<script lang="ts">
  import { onDestroy } from 'svelte';
  import { ensureReady, machines, wake } from '../state/machines';
  import { elapsedS } from '../modules/machines/machines';

  // Hiding only hides: the start carries on. A new wake (other `since`) shows it again.
  let hidden = $state<number | null>(null);
  let now = $state(Date.now());
  const timer = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => clearInterval(timer));

  const name = $derived($wake ? ($machines.find((m) => m.id === $wake.id)?.name ?? 'the machine') : '');
  const failed = $derived($wake?.phase === 'failed');
  const show = $derived(!!$wake && (failed || hidden !== $wake.since));
</script>

{#if $wake && show}
  <div class="wake glass" role={failed ? 'alertdialog' : 'status'} aria-label={failed ? `${name} did not start` : `Starting ${name}`} aria-live="polite">
    {#if failed}
      <strong>{name} did not start</strong>
      <span class="lab">{$wake.error ?? 'It did not come up in time.'}</span>
      <div class="a">
        <button class="btn ghost" onclick={() => (hidden = $wake.since)}>Close</button>
        <button class="btn" onclick={() => void ensureReady($wake.id).catch(() => {})}>Retry</button>
      </div>
    {:else}
      <strong>Starting {name}… (GPU, Steam, Sunshine)</strong>
      <span class="lab">{$wake.phase === 'waiting' ? 'Waiting for it to answer' : 'Powering on'} · {elapsedS($wake.since, now)} s</span>
      <span class="bar" aria-hidden="true"></span>
      <div class="a"><button class="btn ghost" onclick={() => (hidden = $wake.since)}>Hide</button></div>
    {/if}
  </div>
{/if}

<style>
  .wake { position: fixed; left: 0; right: 0; margin-inline: auto; bottom: 24px; z-index: 45; width: min(420px, calc(100vw - 32px)); padding: 16px 18px; display: grid; gap: 6px; }
  .a { display: flex; gap: 8px; justify-content: flex-end; margin-top: 8px; }
  /* indeterminate: a short bar sliding along a track, no percentages */
  .bar { height: 4px; border-radius: 2px; background: var(--panel2); overflow: hidden; position: relative; }
  .bar::after { content: ''; position: absolute; inset: 0 auto 0 0; width: 30%; border-radius: 2px; background: var(--accent); }
  @media (prefers-reduced-motion: no-preference) {
    .bar::after { animation: wake-slide 1.6s var(--ease) infinite; }
    .wake { animation: enter-rise var(--dur-enter) var(--ease) backwards; }
  }
  @keyframes wake-slide { from { translate: -100% 0; } to { translate: 340% 0; } }
</style>
