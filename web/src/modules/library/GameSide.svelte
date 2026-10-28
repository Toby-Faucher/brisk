<script lang="ts">
  // The game panel's static side: art, title, Play / Install / Quit, the update notice, quick facts, achievement
  // progress, automatic tags, and your note and tags. GameDetail owns the data and the scrolling side.
  import { Dialog } from 'bits-ui';
  import type { Game, GameInfo, Achievements } from '../../state/api';
  import QuitLabel from '../../ui/QuitLabel.svelte';
  import Icon from '../../ui/Icon.svelte';
  import PlayIcon from '../../ui/PlayIcon.svelte';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';
  import { hue } from './Cover.svelte';
  import { badgeLabel, installLabel, playLabel, smartTags } from './library';
  import GameNotes from './GameNotes.svelte';
  import TagChip from './TagChip.svelte';

  let { game, busy, info, ach, onplay, oninstall }: { game: Game; busy: boolean; info: GameInfo | null | 'none'; ach: Achievements | null; onplay: (g: Game) => void; oninstall: (g: Game) => void } = $props();

  let heroBad = $state('');
  let logoLoaded = $state('');
  const logoOk = $derived(!!game.logo && logoLoaded === game.logo);
  const hrs = $derived(game.playtimeMin < 60 ? `${game.playtimeMin} min` : `${Math.round(game.playtimeMin / 60)} h`);
  const q = $derived($quitting.has(game.appid));
  const facts = $derived(info && info !== 'none' ? info : null);
</script>

<div class="head" style="background:radial-gradient(120% 140% at 85% 0%, hsl({hue(game)} 62% 48% / .67), #12151c 62%)">
  {#if game.hero && heroBad !== game.hero}<img class="art" src={game.hero} alt="" onerror={() => (heroBad = game.hero)} />{/if}
  {#if game.logo}<img class="logo" class:on={logoOk} src={game.logo} alt="" onload={() => (logoLoaded = game.logo)} />{/if}
  <Dialog.Title class={logoOk ? 'gd-t vh' : 'gd-t'}>{game.name}</Dialog.Title>
  <Dialog.Description class="gd-d">{badgeLabel(game)}{game.playtimeMin ? ` · ${hrs} played` : ''}</Dialog.Description>
</div>

{#if game.installed && game.update}<p class="upd" role="note">Steam still has to update this game. Starting it now may wait for the download.</p>{/if}
<div class="acts">
  {#if game.installed}
    <button class="btn" aria-disabled={busy} onclick={() => onplay(game)}><PlayIcon {busy} size={14} /> {busy ? 'Starting…' : playLabel(game)}</button>
  {:else}
    <button class="btn" aria-disabled={busy} onclick={() => oninstall(game)}><Icon name="dl" size={14} /> {busy ? 'Asking Steam…' : installLabel(game)}</button>
  {/if}
  {#if game.running}
    <button class="btn ghost" aria-disabled={q} aria-busy={q} onclick={() => askQuit(game)} aria-label={q ? `Quitting ${game.name}` : `Quit ${game.name}`}><QuitLabel busy={q} icon={14} label="Quit" /></button>
  {/if}
  <Dialog.Close class="btn ghost close">Close</Dialog.Close>
</div>

{#if facts}
  <section class="sec facts" aria-label="Quick facts">
    {#if facts.controller}<p class="ctl"><Icon name="pad" size={15} /> {facts.controller === 'full' ? 'Full' : 'Partial'} controller support</p>{/if}
    {#if facts.genres.length || facts.coop || facts.remotePlayTogether}
      <ul class="chips" aria-label="Genres and modes">
        {#each facts.genres as g (g)}<li>{g}</li>{/each}
        {#if facts.coop}<li class="mode">Co-op</li>{/if}
        {#if facts.remotePlayTogether}<li class="mode">Remote Play Together</li>{/if}
      </ul>
    {/if}
    {#if facts.release || facts.metacritic}<p class="lab">{[facts.release && `Released ${facts.release}`, facts.metacritic && `Metacritic ${facts.metacritic}`].filter(Boolean).join(' · ')}</p>{/if}
  </section>
{/if}

{#if ach && ach.total > 0}
  <section class="sec" aria-label="Achievement progress">
    <p class="lab">{ach.unlocked} of {ach.total} unlocked</p>
    <div class="bar" role="progressbar" aria-label="{ach.unlocked} of {ach.total} achievements" aria-valuemin={0} aria-valuemax={ach.total} aria-valuenow={ach.unlocked}><i style="width:{(ach.unlocked / ach.total) * 100}%"></i></div>
  </section>
{/if}

<!-- your note and tags (keyed: a fresh draft per game) -->
{#key game.appid}<GameNotes appid={game.appid} name={game.name} />{/key}
{#if smartTags(game).length}
  <section class="sec" aria-label="Automatic tags">
    <h3>Automatic tags</h3>
    <div class="auto">{#each smartTags(game) as t (t.id)}<TagChip name={t.name} color={t.color} symbol={t.symbol} />{/each}</div>
  </section>
{/if}

<style>
  .head { position: relative; overflow: hidden; min-height: 170px; padding: 22px; display: flex; flex-direction: column; justify-content: flex-end; gap: 4px; }
  .head > :global(*:not(.art)) { position: relative; }
  .art { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; object-position: center top; opacity: .55; mask-image: linear-gradient(0deg, transparent, #000 70%); pointer-events: none; }
  .logo { display: none; max-width: 260px; max-height: 84px; object-fit: contain; object-position: left; }
  .logo.on { display: block; }
  :global(.gd-t) { margin: 0; font-size: 24px; letter-spacing: -.03em; }
  :global(.gd-t.vh) { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  :global(.gd-d) { color: #c9d1e0; font-size: 13.5px; }
  .upd { margin: 14px 22px 0; font-size: 13px; color: var(--muted); }
  .acts { display: flex; gap: 8px; flex-wrap: wrap; padding: 14px 22px 0; }
  .acts :global(.close) { margin-left: auto; }
  .sec { padding: 14px 22px 0; display: grid; gap: 10px; }
  .ctl { display: flex; gap: 6px; align-items: center; margin: 0; }
  .facts p { margin: 0; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; list-style: none; padding: 0; margin: 0; }
  .chips li { padding: 3px 10px; border-radius: 999px; font-size: 12.5px; border: 1px solid var(--line); background: var(--panel); }
  .chips li.mode { border-color: color-mix(in srgb, var(--accent) 50%, transparent); }
  .bar { height: 6px; border-radius: 3px; background: var(--line); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); }
  .auto { display: flex; flex-wrap: wrap; gap: 6px; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
</style>
