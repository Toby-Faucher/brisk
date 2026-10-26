<script lang="ts">
  import type { Game } from '../../state/api';
  import Icon from '../../ui/Icon.svelte';
  import { fmtAgo } from '../home/home';
  import Cover, { hue } from './Cover.svelte';

  let { game, busy, onpick }: { game: Game; busy: boolean; onpick: (g: Game) => void } = $props();
  const hrs = $derived(game.playtimeMin < 60 ? `${game.playtimeMin} min` : `${Math.round(game.playtimeMin / 60)} h`);
</script>

<div class="hero" style="background:radial-gradient(120% 140% at 85% 0%, hsl({hue(game)} 62% 48% / .67), #12151c 62%)">
  <Cover {game} style="width:150px;flex:0 0 auto" />
  <div class="txt">
    <div class="lab">Continue where you left off</div>
    <h3>{game.name}</h3>
    <div class="lab">Played {fmtAgo(game.lastPlayed! * 1000)} · {hrs} total</div>
    <button class="btn" aria-disabled={busy} onclick={() => onpick(game)}><Icon name="play" size={14} /> {busy ? 'Starting…' : 'Resume'}</button>
  </div>
</div>

<style>
  .hero { border-radius: 24px; padding: 22px; display: flex; gap: 22px; align-items: center; min-height: 200px; position: relative; overflow: hidden; border: 1px solid var(--line); }
  .txt { min-width: 0; }
  .lab { color: #c9d1e0; font-size: 14px; }
  h3 { margin: 2px 0 6px; font-size: 28px; letter-spacing: -.03em; }
  .btn { margin-top: 16px; }
</style>
