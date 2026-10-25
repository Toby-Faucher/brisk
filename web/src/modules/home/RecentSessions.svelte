<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type SessionRec } from '../../state/api';
  import { games } from '../../state/games';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { DASH, fmtAgo, fmtDuration } from './home';

  let list: SessionRec[] | null = $state(null);
  onMount(() => {
    api.sessions().then((r) => (list = r.sessions.slice(0, 4)), () => (list = null));
  });
  const cover = (appid: number | null) => $games?.games.find((g) => g.appid === appid)?.cover;
</script>

<Card cols={2} rows={2}>
  <h4><Icon name="lib" color="var(--c-lib)" size={16} /> Recent sessions</h4>
  {#each list ?? [] as s (s.started)}
    {@const c = cover(s.appid)}
    <div class="mini">
      {#if c}<img class="cover" src={c} alt="" />{:else}<div class="cover"></div>{/if}
      <div class="txt"><b>{s.name}</b><div class="lab">{fmtDuration(s.started, s.ended)} · {fmtAgo(s.started)}</div></div>
    </div>
  {:else}
    <div class="empty"><b>{DASH}</b><div class="lab">{list ? 'No sessions yet' : 'Sessions are not available here'}</div></div>
  {/each}
</Card>

<style>
  .mini { display: flex; align-items: center; gap: 10px; padding: 7px 0; border-top: 1px solid color-mix(in srgb, #fff 8%, transparent); }
  h4 + .mini { border-top: 0; margin-top: 6px; }
  .cover { width: 30px; height: 30px; border-radius: 9px; flex: 0 0 auto; object-fit: cover; background: linear-gradient(135deg, var(--panel2), var(--line)); }
  .txt { min-width: 0; }
  .txt b { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { margin-top: 12px; }
</style>
