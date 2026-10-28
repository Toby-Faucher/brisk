<script lang="ts">
  // Every achievement of one game over the game panel, with a small search on name and description (case and accents
  // ignored, as the Library search). Locked ones are dimmed. Focus goes back to the View all button.
  import { Dialog } from 'bits-ui';
  import type { Achievements } from '../../state/api';
  import { fold } from './library';

  let { ach, name, onclose }: { ach: Achievements; name: string; onclose: () => void } = $props();

  let open = $state(true);
  let q = $state('');
  const back = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const folded = $derived(ach.list.map((a) => ({ a, k: fold(`${a.name} ${a.description}`) })));
  const shown = $derived.by(() => {
    const fq = fold(q);
    return fq ? folded.filter((x) => x.k.includes(fq)).map((x) => x.a) : ach.list;
  });
  const day = (s: number) => new Date(s * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
</script>

<Dialog.Root bind:open onOpenChangeComplete={(o) => !o && onclose()}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim ad-dim" />
    <Dialog.Content
      class="rs glass ad"
      onCloseAutoFocus={(e) => {
        if (back?.isConnected) {
          e.preventDefault();
          back.focus();
        }
      }}
    >
      <div class="top">
        <Dialog.Title class="rs-t">{name} achievements</Dialog.Title>
        <Dialog.Description class="lab">{ach.unlocked} of {ach.total} unlocked</Dialog.Description>
        <!-- Escape with text clears it (as the Library search); on an empty box it closes the dialog -->
        <input type="search" bind:value={q} placeholder="Search" aria-label="Search achievements" autocomplete="off" onkeydown={(e) => e.key === 'Escape' && q && (e.preventDefault(), e.stopPropagation(), (q = ''))} />
      </div>
      {#if shown.length}
        <!-- focusable so the keyboard (and Safari) can scroll it: a scrolling region needs a tab stop (axe scrollable-region-focusable) -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <ul tabindex="0" aria-label="Achievements">
          {#each shown as a (a.id)}
            <li class:locked={a.unlockedAt === null}>
              {#if a.icon}<img src={a.icon} alt="" loading="lazy" decoding="async" />{:else}<span class="ph"></span>{/if}
              <span class="t">
                <b>{a.name}</b>
                {#if a.hardcore}<small class="hc">Hardcore</small>{/if}
                {#if a.description}<small>{a.description}</small>{/if}
                <small class="when">{a.unlockedAt === null ? 'Locked' : `Unlocked ${day(a.unlockedAt)}`}</small>
              </span>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="lab none" role="status">No matches</p>
      {/if}
      <div class="rs-a"><Dialog.Close class="btn ghost">Close</Dialog.Close></div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.cm-dim.ad-dim) { z-index: 42; }
  :global(.rs.ad) { position: fixed; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(640px, calc(100vw - 32px)); max-height: calc(100dvh - 32px); z-index: 43; display: flex; flex-direction: column; gap: 10px; }
  .top { display: grid; gap: 4px; }
  input { margin-top: 6px; width: min(220px, 100%); font: inherit; font-size: 13px; color: var(--fg); background: var(--panel); border: 1px solid var(--line); border-radius: 9px; padding: 5px 10px; }
  ul { list-style: none; margin: 0; padding: 0 4px 0 0; display: grid; gap: 10px; overflow: auto; overscroll-behavior: contain; min-height: 0; }
  li { display: flex; gap: 12px; align-items: center; }
  /* locked: the icon fades and the name goes grey; the text keeps its contrast */
  li.locked :is(img, .ph) { opacity: .4; filter: grayscale(1); }
  li.locked b { color: var(--muted); font-weight: 500; }
  img, .ph { width: 44px; height: 44px; flex: none; border-radius: 8px; background: var(--panel); }
  .t { display: grid; min-width: 0; }
  small { color: var(--muted); }
  .when { font-size: 11.5px; }
  .none { padding: 18px 0; text-align: center; }
  .rs-a { margin-top: 4px; }
</style>
