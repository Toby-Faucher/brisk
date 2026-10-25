<script lang="ts" module>
  import { writable } from 'svelte/store';
  import { play } from '../ui/sounds';

  type Toast = { id: number; text: string; error: boolean };
  const list = writable<Toast[]>([]);
  let next = 0;

  /** Short message at the bottom; errors also play the error tone. */
  export function toast(text: string, opts: { error?: boolean; ms?: number } = {}) {
    const t = { id: next++, text, error: !!opts.error };
    list.update((l) => [...l, t]);
    if (t.error) play('error');
    setTimeout(() => list.update((l) => l.filter((x) => x !== t)), opts.ms ?? 3500);
  }
</script>

<script lang="ts">
  import { fly } from 'svelte/transition';
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each $list as t (t.id)}
    <div class="toast glass" class:error={t.error} transition:fly={{ y: 14, duration: 300 }}>{t.text}</div>
  {/each}
</div>

<style>
  .toasts { position: fixed; left: 50%; bottom: 22px; transform: translateX(-50%); display: grid; gap: 8px; justify-items: center; z-index: 100; pointer-events: none; }
  .toast { padding: 9px 16px; border-radius: 14px; font-weight: 500; }
  .toast.error { color: var(--bad); }
</style>
