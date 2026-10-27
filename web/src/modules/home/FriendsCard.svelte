<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type Friends } from '../../state/api';
  import { pollVisible } from '../../state/host';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';

  let { poll = pollVisible }: { poll?: typeof pollVisible } = $props();
  let f: Friends | null = $state(null);
  // Names are other people's: shown as text, kept in memory only.
  onMount(() => poll(() => void api.friends().then((r) => (f = r), () => (f = null)), document, 60_000));
</script>

<Card cols={2} rows={2}>
  <h4><Icon name="agent" color="var(--c-agent)" size={16} /> Friends online</h4>
  {#if f?.available && f.online.length}
    <ul>
      {#each f.online as p, i (i)}
        <li>
          {#if p.avatar}<img class="av" src={p.avatar} alt="" />{:else}<span class="av" aria-hidden="true">{[...p.name][0]?.toUpperCase()}</span>{/if}
          <div class="txt"><b>{p.name}</b><div class="lab">{p.playing ? `playing ${p.playing}` : p.state}</div></div>
        </li>
      {/each}
    </ul>
  {:else}
    <div class="empty lab">{f === null ? 'Friends are not available here' : !f.available ? 'Friends are not available' : 'No one online'}</div>
  {/if}
</Card>

<style>
  ul { list-style: none; margin: 6px 0 0; padding: 0; overflow-y: auto; max-height: 200px; }
  li { display: flex; align-items: center; gap: 10px; padding: 6px 0; }
  .av { width: 28px; height: 28px; border-radius: 50%; flex: 0 0 auto; object-fit: cover; display: grid; place-items: center; font-size: 12px; background: linear-gradient(135deg, var(--panel2), var(--line)); }
  .txt { min-width: 0; }
  .txt b { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { margin-top: 12px; }
</style>
