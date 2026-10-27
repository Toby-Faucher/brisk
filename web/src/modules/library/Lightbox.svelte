<script lang="ts">
  // A screenshot at full size over the game panel: previous / next (buttons and arrow keys, wrapping), Escape or Close
  // to leave. Focus is trapped by Bits and goes back to the thumbnail that opened it. `src` '' = still loading, or
  // `failed`.
  import { Dialog } from 'bits-ui';
  import Icon from '../../ui/Icon.svelte';

  let { shots, index = $bindable(), onclose }: { shots: { src: string; alt: string; failed?: boolean }[]; index: number; onclose: () => void } = $props();

  let open = $state(true);
  const back = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const n = $derived(shots.length);
  const cur = $derived(shots[index]);
  const step = (d: number) => (index = (index + d + n) % n);
  const keys = (e: KeyboardEvent) => {
    const d = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
    if (!d || n < 2) return;
    e.preventDefault();
    step(d);
  };
</script>

<Dialog.Root bind:open onOpenChangeComplete={(o) => !o && onclose()}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim lb-dim" />
    <Dialog.Content
      class="rs lb"
      onkeydown={keys}
      onCloseAutoFocus={(e) => {
        if (back?.isConnected) {
          e.preventDefault();
          back.focus();
        }
      }}
    >
      <div class="bar">
        <Dialog.Title class="lab lb-t">Screenshot {index + 1} of {n}</Dialog.Title>
        <Dialog.Close class="btn ghost" aria-label="Close screenshot">Close</Dialog.Close>
      </div>
      <figure>
        {#if cur.src}<img src={cur.src} alt={cur.alt} />{:else}<p class="lab" role="status">{cur.failed ? 'Could not load this screenshot' : 'Loading…'}</p>{/if}
        {#if n > 1}
          <button class="nav prev" aria-label="Previous screenshot" onclick={() => step(-1)}><Icon name="chev" size={20} /></button>
          <button class="nav next" aria-label="Next screenshot" onclick={() => step(1)}><Icon name="chev" size={20} /></button>
        {/if}
      </figure>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  /* above the game panel (41), like the tag editor */
  :global(.cm-dim.lb-dim) { z-index: 42; background: rgba(0, 0, 0, .82); }
  :global(.rs.lb) { position: fixed; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(1280px, calc(100vw - 32px)); padding: 0; z-index: 43; display: grid; gap: 8px; }
  .bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  :global(.lb .lb-t) { font-size: 13px; font-weight: 500; }
  figure { position: relative; margin: 0; min-height: 200px; display: grid; place-items: center; }
  img { display: block; max-width: 100%; max-height: calc(100dvh - 96px); border-radius: 12px; object-fit: contain; background: var(--panel); }
  .nav { position: absolute; top: 50%; translate: 0 -50%; width: 44px; height: 44px; border-radius: 50%; border: 1px solid var(--line); background: color-mix(in srgb, #12151b 80%, transparent); color: var(--fg); display: grid; place-items: center; cursor: pointer; padding: 0; }
  .nav:hover { background: var(--panel2); }
  .prev { left: 8px; }
  .prev :global(svg) { transform: scaleX(-1); }
  .next { right: 8px; }
</style>
