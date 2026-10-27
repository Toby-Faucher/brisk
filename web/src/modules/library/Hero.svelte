<script lang="ts">
  import type { Game } from '../../state/api';
  import { prefs, prefsChip } from '../../state/prefs';
  import Icon from '../../ui/Icon.svelte';
  import PlayIcon from '../../ui/PlayIcon.svelte';
  import Tip from '../../ui/Tip.svelte';
  import { fmtAgo } from '../home/home';
  import Cover, { hue } from './Cover.svelte';
  import { playLabel } from './library';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';

  let { game, busy, onplay, ondetails, titleIn }: { game: Game; busy: boolean; onplay: (g: Game) => void; ondetails: (g: Game) => void; titleIn?: (h: HTMLElement) => void | (() => void) } = $props();
  // Hero art 404s for some games: remember the url that failed (or loaded) and fall back to the wash alone.
  let heroBad = $state('');
  let logoLoaded = $state('');
  const heroOk = $derived(!!game.hero && heroBad !== game.hero);
  const logoOk = $derived(!!game.logo && logoLoaded === game.logo);
  const saved = $derived($prefs[game.appid]);
  const hrs = $derived(game.playtimeMin < 60 ? `${game.playtimeMin} min` : `${Math.round(game.playtimeMin / 60)} h`);
</script>

<div class="hero reveal" style="background:radial-gradient(120% 140% at 85% 0%, hsl({hue(game)} 62% 48% / .67), #12151c 62%)">
  {#if heroOk}
    <img src={game.hero} alt="" hidden onerror={() => (heroBad = game.hero)} />
    <div class="art" style="background-image:url({game.hero})"></div>
  {/if}
  <Cover {game} style="width:150px;flex:0 0 auto" />
  <div class="txt">
    <div class="lab">{game.running ? 'Running on the host' : 'Continue where you left off'}</div>
    {#if game.logo}<img class="logo" class:on={logoOk} src={game.logo} alt="" onload={() => (logoLoaded = game.logo)} />{/if}
    <h3 class:vh={logoOk} {@attach titleIn}><span>{game.name}</span></h3>
    <div class="lab">Played {fmtAgo(game.lastPlayed! * 1000)} · {hrs} total{#if saved} <span class="pchip">{prefsChip(saved)}</span><Tip label="Saved quality for this game" />{/if}</div>
    <div class="acts">
      <button class="btn" aria-disabled={busy} onclick={() => onplay(game)}><PlayIcon {busy} size={14} /> {busy ? 'Starting…' : playLabel(game)}</button>
      <button class="btn ghost" aria-haspopup="dialog" onclick={() => ondetails(game)} aria-label="Details for {game.name}">Details</button>
      {#if game.running}
        {@const q = $quitting.has(game.appid)}
        <button class="btn ghost" aria-disabled={q} onclick={() => askQuit(game)} aria-label={q ? `Quitting ${game.name}` : `Quit ${game.name}`}><Icon name="power" size={14} /> {q ? 'Quitting…' : 'Quit'}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .hero { border-radius: 24px; padding: 22px; display: flex; gap: 22px; align-items: center; min-height: 200px; position: relative; overflow: hidden; border: 1px solid var(--line); }
  .art { position: absolute; inset: 0; background-size: cover; background-position: center top; opacity: .5; mask-image: linear-gradient(90deg, transparent 20%, #000); pointer-events: none; }
  .hero > :global(*:not(.art)) { position: relative; }
  .txt { min-width: 0; }
  .logo { display: none; max-width: 280px; max-height: 90px; object-fit: contain; object-position: left; }
  .logo.on { display: block; }
  h3.vh { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .lab { color: #c9d1e0; font-size: 14px; }
  h3 { margin: 2px 0 6px; font-size: 28px; letter-spacing: -.03em; position: relative; }
  h3 :global(.split-fx) { position: absolute; inset: 0; user-select: none; pointer-events: none; }
  h3 :global(.split-fx .w) { display: inline-block; overflow: clip; }
  h3 :global(.split-fx .wi) { display: inline-block; }
  .pchip { margin-left: 6px; padding: 1px 8px; border-radius: 999px; font-size: 11.5px; color: #fff; background: #000b; white-space: nowrap; }
  .acts { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 16px; }
</style>
