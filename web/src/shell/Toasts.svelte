<script lang="ts" module>
  import { writable } from 'svelte/store';
  import { play } from '../ui/sounds';
  import type { ToastOpts } from '../state/quit';

  type Toast = { id: number; text: string; error: boolean; thumb?: string; action?: ToastOpts['action'] };
  const list = writable<Toast[]>([]);
  let next = 0;

  /** Short message at the bottom; errors also play the error tone. An `action` adds one button that dismisses the toast. */
  export function toast(text: string, opts: ToastOpts = {}) {
    const t = { id: next++, text, error: !!opts.error, thumb: opts.thumb, action: opts.action };
    list.update((l) => [...l, t]);
    if (t.error) play('error');
    setTimeout(() => list.update((l) => l.filter((x) => x !== t)), opts.ms ?? 3500);
  }
</script>

<script lang="ts">
  import { fly } from 'svelte/transition';
  const act = (t: Toast) => {
    list.update((l) => l.filter((x) => x !== t));
    t.action?.run();
  };
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each $list as t (t.id)}
    <div class="toast glass" class:error={t.error} transition:fly={{ y: 14, duration: 300 }}>
      {#if t.thumb}<img class="thumb" src={t.thumb} alt="" />{/if}
      {t.text}
      {#if t.action}<button class="btn ghost act" onclick={() => act(t)}>{t.action.label}</button>{/if}
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; left: 50%; bottom: 22px; transform: translateX(-50%); display: grid; gap: 8px; justify-items: center; z-index: 100; pointer-events: none; }
  .toast { padding: 9px 16px; border-radius: 14px; font-weight: 500; display: flex; gap: 12px; align-items: center; }
  .toast.error { color: var(--bad); }
  .thumb { height: 36px; width: auto; max-width: 64px; object-fit: cover; border-radius: 6px; }
  .act { pointer-events: auto; padding: 4px 10px; font-size: 13px; }
</style>
