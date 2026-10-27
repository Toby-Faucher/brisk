<script lang="ts">
  // Make or edit a tag: name, color, symbol and its own note. Delete asks first and says how many games use it.
  import { Dialog } from 'bits-ui';
  import { api } from '../../state/api';
  import { newTagId, notes, tagUses, TAG_COLORS, TAG_SYMBOLS, withoutTag, type Tag, type TagColor } from '../../state/notes';
  import { toast } from '../../shell/Toasts.svelte';
  import TagChip from './TagChip.svelte';

  /** `id` null: a new tag. `onsaved` gets the id once hostd has stored it; `ondeleted` places focus (the trigger is gone). */
  let { id, onclose, onsaved, ondeleted }: { id: string | null; onclose: () => void; onsaved?: (id: string) => void; ondeleted?: () => void } = $props();

  const COLORS = Object.keys(TAG_COLORS) as TagColor[];
  // the form starts from the tag as it is when the editor opens
  const initial = (): Tag => (id && $notes.tags[id]) || { name: '', color: 'teal', symbol: 'star', note: '', pinned: false };
  const start = initial();
  let name = $state(start.name);
  let color = $state<TagColor>(start.color);
  let symbol = $state(start.symbol);
  let note = $state(start.note);
  let pinned = $state(!!start.pinned);
  let open = $state(true);
  let confirming = $state(false);
  let busy = $state(false);
  let deleted = false;
  const keepFocus = (e: Event) => deleted && e.preventDefault(); // ondeleted places it once the editor is gone
  const uses = $derived(id ? tagUses($notes, id) : 0);

  /** Arrow keys move the choice in a radio group (roving tabindex), as Settings' accent swatches do. */
  function arrow<T extends string>(e: KeyboardEvent, list: T[], cur: T, set: (v: T) => void) {
    const d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!d) return;
    e.preventDefault();
    const v = list[(list.indexOf(cur) + d + list.length) % list.length];
    set(v);
    (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`[data-v="${v}"]`)?.focus();
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !name.trim()) return;
    busy = true;
    const tid = id ?? newTagId();
    try {
      const t = await api.putTag(tid, { name: name.trim(), color, symbol, note, pinned });
      notes.update((n) => ({ ...n, tags: { ...n.tags, [tid]: t } }));
      onsaved?.(tid);
      open = false;
    } catch (err) {
      toast(`Could not save the tag: ${(err as Error).message}`, { error: true });
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!id || busy) return;
    busy = true;
    try {
      await api.deleteTag(id);
      notes.update((n) => withoutTag(n, id));
      deleted = true;
      confirming = false;
      open = false;
    } catch (err) {
      toast(`Could not delete the tag: ${(err as Error).message}`, { error: true });
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root bind:open onOpenChangeComplete={(o) => !o && (onclose(), deleted && ondeleted?.())}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim te-dim" />
    <Dialog.Content class="rs glass te" onCloseAutoFocus={keepFocus}>
      <Dialog.Title class="rs-t">{id ? 'Edit tag' : 'New tag'}</Dialog.Title>
      <form onsubmit={save}>
        <label class="f">Name <input bind:value={name} maxlength={24} required autocomplete="off" /></label>
        <div class="f">
          <span id="te-c">Color</span>
          <div class="row" role="radiogroup" aria-labelledby="te-c">
            {#each COLORS as c (c)}
              <button type="button" class="sw" data-v={c} style:background={TAG_COLORS[c]} role="radio" aria-checked={color === c} aria-label={c} tabindex={color === c ? 0 : -1} onkeydown={(e) => arrow(e, COLORS, color, (v) => (color = v))} onclick={() => (color = c)}></button>
            {/each}
          </div>
        </div>
        <div class="f">
          <span id="te-s">Symbol</span>
          <div class="row" role="radiogroup" aria-labelledby="te-s">
            {#each TAG_SYMBOLS as s, i (s)}
              <button type="button" class="sy" data-v={s} role="radio" aria-checked={symbol === s} aria-label={s} tabindex={symbol === s || (i === 0 && !TAG_SYMBOLS.includes(symbol)) ? 0 : -1} onkeydown={(e) => arrow(e, TAG_SYMBOLS, symbol, (v) => (symbol = v))} onclick={() => (symbol = s)}><TagChip {color} symbol={s} bare size={16} /></button>
            {/each}
          </div>
        </div>
        <label class="f">Note <textarea bind:value={note} maxlength={500} rows="3"></textarea></label>
        <label class="pin"><input type="checkbox" bind:checked={pinned} /> Show on Home</label>
        <p class="prev" aria-hidden="true"><TagChip name={name.trim() || 'Tag'} {color} {symbol} /></p>
        <div class="rs-a">
          {#if id}<button type="button" class="btn ghost del" onclick={() => (confirming = true)}>Delete tag</button>{/if}
          <Dialog.Close type="button" class="btn ghost">Cancel</Dialog.Close>
          <button type="submit" class="btn" disabled={!name.trim()} aria-busy={busy}>Save tag</button>
        </div>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={confirming}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim te-dim2" />
    <Dialog.Content class="rs glass te2" onCloseAutoFocus={keepFocus}>
      <Dialog.Title class="rs-t">Delete tag {name.trim() || start.name}?</Dialog.Title>
      <Dialog.Description class="lab">{uses === 0 ? 'No game uses it.' : `It is on ${uses} ${uses === 1 ? 'game' : 'games'}. Their notes stay.`}</Dialog.Description>
      <div class="rs-a">
        <Dialog.Close class="btn ghost">Cancel</Dialog.Close>
        <button class="btn" onclick={remove}>Delete</button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.rs.te) { width: min(420px, calc(100vw - 32px)); z-index: 43; }
  /* above the detail panel (41), and the delete confirm above this */
  :global(.cm-dim.te-dim) { z-index: 42; }
  :global(.cm-dim.te-dim2) { z-index: 44; }
  :global(.rs.te2) { z-index: 45; }
  form { display: grid; gap: 12px; margin-top: 8px; }
  .f { display: grid; gap: 6px; font-size: 13px; color: var(--muted); }
  .f input, .f textarea { font: inherit; font-size: 14px; color: var(--fg); background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 7px 10px; resize: vertical; }
  .row { display: flex; flex-wrap: wrap; gap: 6px; }
  .sw { width: 26px; height: 26px; border-radius: 50%; border: 2px solid transparent; cursor: pointer; }
  .sw[aria-checked='true'] { outline: 2px solid var(--fg); outline-offset: 2px; }
  .sy { width: 32px; height: 32px; display: grid; place-items: center; border-radius: 9px; border: 1px solid var(--line); background: var(--panel); cursor: pointer; }
  .sy[aria-checked='true'] { border-color: var(--fg); background: var(--panel2); }
  .pin { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--muted); }
  .pin input { accent-color: var(--accent); width: 18px; height: 18px; }
  .prev { margin: 0; }
  .del { margin-right: auto; }
</style>
