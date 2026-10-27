<script lang="ts">
  // Your note and tags for one game, in the detail panel. The note saves 800 ms after you stop typing, on blur and
  // when the panel closes; a failed save keeps the text and tries again on the next edit or on focus.
  import { onMount } from 'svelte';
  import { gameNoteSaver as saver, gameTags, notes } from '../../state/notes';
  import TagChip from './TagChip.svelte';
  import TagEditor from './TagEditor.svelte';

  let { appid, name }: { appid: number; name: string } = $props();

  const status = saver.status;
  let draft = $state('');
  let touched = false;
  // the store can arrive after the panel opened: follow it until you type
  $effect(() => {
    const n = $notes.games[appid]?.note ?? '';
    if (!touched) draft = n;
  });
  const tags = $derived(gameTags($notes, appid));
  const others = $derived(Object.entries($notes.tags).filter(([id]) => !tags.some((t) => t.id === id)));
  let picking = $state(false);
  /** The tag in the editor: undefined closed, null a new one. */
  let editing = $state<string | null | undefined>(undefined);

  const save = (ids: string[]) => saver.save(appid, { note: draft, tags: ids });
  const ids = () => tags.map((t) => t.id);
  function setTags(next: string[]) {
    picking = false;
    save(next);
    void saver.flush();
  }

  onMount(() => {
    if ($status === 'saved') status.set('idle'); // that was another game, or an earlier visit
    const retry = () => void saver.retry();
    const flush = () => void saver.flush();
    addEventListener('focus', retry);
    addEventListener('pagehide', flush);
    return () => {
      removeEventListener('focus', retry);
      removeEventListener('pagehide', flush);
      flush(); // the panel closed
    };
  });

  const STATUS = { idle: '', saving: 'Saving…', saved: 'Saved', failed: 'Not saved, will retry' };
</script>

<section class="gn">
  <h3>Notes</h3>
  <textarea
    bind:value={draft}
    aria-label="Note for {name}"
    placeholder="Anything to remember about this game"
    maxlength={4000}
    rows="3"
    oninput={() => ((touched = true), save(ids()))}
    onblur={() => void saver.flush()}
    onfocus={() => void saver.retry()}
  ></textarea>
  <p class="st lab" aria-live="polite">{STATUS[$status]}</p>

  <ul class="tags" aria-label="Tags">
    {#each tags as t (t.id)}
      <li>
        <button class="tb" aria-label="Edit tag {t.name}" onclick={() => (editing = t.id)}><TagChip name={t.name} color={t.color} symbol={t.symbol} /></button>
        <button class="x" aria-label="Remove tag {t.name}" onclick={() => setTags(ids().filter((i) => i !== t.id))}>×</button>
      </li>
    {/each}
    {#if tags.length < 16}
      <li><button class="btn ghost add" aria-label="Add a tag" aria-expanded={picking} onclick={() => (picking = !picking)}>+ Tag</button></li>
    {/if}
  </ul>
  {#if picking}
    <div class="pick">
      {#each others as [id, t] (id)}<button class="tb" onclick={() => setTags([...ids(), id])}><TagChip name={t.name} color={t.color} symbol={t.symbol} /></button>{/each}
      <button class="btn ghost" onclick={() => ((picking = false), (editing = null))}>New tag…</button>
    </div>
  {/if}
  {#if tags.some((t) => t.note)}
    <ul class="tnotes">
      {#each tags.filter((t) => t.note) as t (t.id)}<li><b>{t.name}:</b> <span>{t.note}</span></li>{/each}
    </ul>
  {/if}
</section>

{#if editing !== undefined}
  <TagEditor id={editing} onclose={() => (editing = undefined)} onsaved={(id) => editing === null && setTags([...ids(), id])} />
{/if}

<style>
  .gn { padding: 14px 22px 0; display: grid; gap: 8px; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
  textarea { font: inherit; color: var(--fg); background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 8px 10px; resize: vertical; min-height: 64px; white-space: pre-wrap; }
  .st { min-height: 1.2em; margin: 0; font-size: 12.5px; }
  .tags, .tnotes { list-style: none; padding: 0; margin: 0; display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .tags li { display: flex; align-items: center; }
  .tb { padding: 0; border: 0; background: none; cursor: pointer; border-radius: 999px; }
  .x { border: 0; background: none; color: var(--muted); cursor: pointer; min-width: 24px; min-height: 24px; font-size: 15px; }
  .x:hover { color: var(--fg); }
  .add { padding: 3px 10px; font-size: 12.5px; min-height: 24px; }
  .pick { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; padding: 8px; border-radius: 12px; border: 1px solid var(--line); background: var(--panel); }
  .pick .btn { padding: 3px 10px; font-size: 12.5px; min-height: 24px; }
  .tnotes { display: grid; gap: 4px; font-size: 13px; color: var(--muted); white-space: pre-wrap; }
  .tnotes b { color: var(--fg); font-weight: 500; }
</style>
