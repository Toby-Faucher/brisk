<script lang="ts">
  import type { Game } from '../../state/api';
  import Icon from '../../ui/Icon.svelte';
  import { fmtAgo } from '../home/home';
  import Cover, { hue } from './Cover.svelte';
  import { playLabel } from './library';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';

  let { game, busy, onpick }: { game: Game; busy: boolean; onpick: (g: Game) => void } = $props();
  const hrs = $derived(game.playtimeMin < 60 ? `${game.playtimeMin} min` : `${Math.round(game.playtimeMin / 60)} h`);
</script>

<div class="hero" style="background:radial-gradient(120% 140% at 85% 0%, hsl({hue(game)} 62% 48% / .67), #12151c 62%)">
  <Cover {game} style="width:150px;flex:0 0 auto" />
  <div class="txt">
    <div class="lab">{game.running ? 'Running on the host' : 'Continue where you left off'}</div>
    <h3>{game.name}</h3>
    <div class="lab">Played {fmtAgo(game.lastPlayed! * 1000)} · {hrs} total</div>
    <div class="acts">
      <button class="btn" aria-disabled={busy} onclick={() => onpick(game)}><Icon name="play" size={14} /> {busy ? 'Starting…' : playLabel(game)}</button>
      {#if game.running}
        {@const q = $quitting.has(game.appid)}
        <button class="btn ghost" aria-disabled={q} onclick={() => askQuit(game)} aria-label={q ? `Quitting ${game.name}` : `Quit ${game.name}`}><Icon name="power" size={14} /> {q ? 'Quitting…' : 'Quit'}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .hero { border-radius: 24px; padding: 22px; display: flex; gap: 22px; align-items: center; min-height: 200px; position: relative; overflow: hidden; border: 1px solid var(--line); }
  .txt { min-width: 0; }
  .lab { color: #c9d1e0; font-size: 14px; }
  h3 { margin: 2px 0 6px; font-size: 28px; letter-spacing: -.03em; }
  .acts { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 16px; }
</style>
