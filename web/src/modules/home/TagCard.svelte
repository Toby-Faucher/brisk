<script lang="ts">
  import { games } from '../../state/games';
  import { notes } from '../../state/notes';
  import Card from '../../ui/Card.svelte';
  import { filterByTag } from '../library/library';
  import TagChip from '../library/TagChip.svelte';

  let { tagId }: { tagId: string } = $props();
  const tag = $derived($notes.tags[tagId]);
  const list = $derived(
    filterByTag($games?.games ?? [], $notes, tagId).toSorted((a, b) => Number(b.installed) - Number(a.installed) || a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })),
  );
  const label = $derived(`${tag?.name}, ${list.length} ${list.length === 1 ? 'game' : 'games'}`);
</script>

{#if tag?.pinned}
  <Card cols={2} rows={2}>
    <a href="/library?tag={tagId}" class="fill" aria-label={label}></a>
    <h4><TagChip color={tag.color} symbol={tag.symbol} name={tag.name} /></h4>
    {#if tag.note.trim()}<p class="note">{tag.note}</p>{/if}
    {#if list.length}
      <div class="covers">{#each list.slice(0, 8) as g (g.appid)}<img src={g.cover} alt="" loading="lazy" decoding="async" />{/each}</div>
      <div class="lab">{list.length} {list.length === 1 ? 'game' : 'games'}</div>
    {:else}
      <div class="lab">No games yet</div>
    {/if}
  </Card>
{/if}

<style>
  .fill { position: absolute; inset: 0; }
  /* three lines at most; text only (never HTML) */
  .note { margin: 8px 0 0; font-size: 13px; color: var(--muted); white-space: pre-line; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow-wrap: anywhere; }
  .covers { display: flex; gap: 6px; margin: 10px 0 6px; }
  .covers img { width: 36px; height: 54px; border-radius: 6px; object-fit: cover; flex: 0 0 auto; background: linear-gradient(135deg, var(--panel2), var(--line)); }
</style>
