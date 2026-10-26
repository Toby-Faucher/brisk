<script lang="ts">
  import { onMount } from 'svelte';
  import { navigate } from 'astro:transitions/client';
  import { games } from '../../state/games';
  import { startGame, watchPrefs } from '../../state/prefs';
  import { toast } from '../../shell/Toasts.svelte';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { playLabel } from '../library/library';
  import { DASH, fmtAgo, pickResume } from './home';

  const g = $derived(pickResume($games?.games ?? []));
  let busy = $state(false);
  onMount(() => watchPrefs());

  async function play() {
    if (!g) return;
    busy = true;
    try {
      await startGame(g.appid);
      await navigate('/stream');
    } catch (e) {
      toast(`Could not start ${g.name}: ${(e as Error).message}`, { error: true });
    } finally {
      busy = false;
    }
  }
</script>

<Card cols={3}>
  <h4><Icon name="stream" color="var(--c-stream)" size={16} /> Resume</h4>
  <div class="row">
    {#if g}<img class="cover" src={g.cover} alt="" />{:else}<div class="cover"></div>{/if}
    <div class="txt">
      <b>{g?.name ?? DASH}</b>
      <div class="lab">{g ? (g.running ? 'Running' : `Played ${fmtAgo(g.lastPlayed! * 1000)}`) : 'Last game is not available here'}</div>
    </div>
    <button class="btn" disabled={!g || busy} onclick={play}><Icon name="play" size={14} /> {g ? playLabel(g) : 'Play'}</button>
  </div>
</Card>

<style>
  .row { display: flex; gap: 12px; align-items: center; margin-top: 12px; }
  .cover { width: 48px; height: 48px; border-radius: 12px; flex: 0 0 auto; object-fit: cover; background: linear-gradient(135deg, var(--panel2), var(--line)); }
  .txt { min-width: 0; }
  .txt b { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .btn { margin-left: auto; }
</style>
