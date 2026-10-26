<script lang="ts">
  import type { Game } from '../../state/api';
  import { prefs, prefsChip } from '../../state/prefs';
  import Icon from '../../ui/Icon.svelte';
  import { fmtAgo } from '../home/home';
  import Cover from './Cover.svelte';
  import { badgeLabel, fmtSize, installLabel, playLabel } from './library';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';

  let { game, busy = false, selected = false, onpick, oninstall }: { game: Game; busy?: boolean; selected?: boolean; onpick: (g: Game) => void; oninstall: (g: Game) => void } = $props();
  const saved = $derived($prefs[game.appid]);
  const size = $derived(fmtSize(game.sizeBytes));
  const played = $derived(game.lastPlayed ? `Played ${fmtAgo(game.lastPlayed * 1000)}` : 'Ready to play');
  const sub = $derived(game.installed ? played : size ? `Not installed · ${size}` : 'Not installed');
  const label = $derived(
    (game.installed ? (game.running ? `Resume ${game.name}, running on the host` : `Play ${game.name}, ${played.toLowerCase()}`) : `${game.name}, not installed${size ? `, ${size}` : ''}`) +
      (saved ? `, saved quality ${prefsChip(saved)}` : ''),
  );
</script>

<div class="wrap">
  <!-- aria-disabled (not disabled) keeps focus on the card while its stream starts -->
  <button class="g" class:off={!game.installed} class:sel={selected} aria-disabled={busy} aria-expanded={game.installed ? undefined : selected} onclick={() => onpick(game)} aria-label={label}>
    <Cover {game}>
      <span class="badge" class:off={!game.installed} class:run={game.running}><i></i>{badgeLabel(game)}</span>
      {#if saved}<span class="pchip" title="Saved quality for this game">{prefsChip(saved)}</span>{/if}
      <span class="glow"></span>
      {#if game.installed}<span class="btn play"><Icon name="play" size={13} /> {busy ? 'Starting…' : playLabel(game)}</span>{/if}
    </Cover>
    <span class="name">{game.name}</span>
    <span class="sub">{sub}</span>
  </button>
  {#if game.running}
    {@const q = $quitting.has(game.appid)}
    <button class="btn ghost quit" aria-disabled={q} onclick={() => askQuit(game)} aria-label={q ? `Quitting ${game.name}` : `Quit ${game.name}`} title="Quit"><Icon name="power" size={13} />{q ? ' Quitting…' : ''}</button>
  {/if}
  {#if selected}
    <div class="over"><button class="btn install" aria-disabled={busy} onclick={() => oninstall(game)}><Icon name="dl" size={13} /> {busy ? 'Asking Steam…' : installLabel(game)}</button></div>
  {/if}
</div>

<style>
  .wrap { position: relative; }
  .g { display: block; width: 100%; text-align: left; padding: 0; border: 0; background: none; cursor: pointer; transition: transform .3s var(--ease); border-radius: 16px; }
  .g:hover, .g:focus-visible, .g.sel { transform: translateY(-5px) scale(1.02); }
  .g[aria-disabled='true'] { cursor: progress; }
  .name { display: block; margin-top: 8px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { display: block; color: var(--muted); font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { position: absolute; left: 10px; top: 10px; z-index: 2; font-size: 11px; font-weight: 600; padding: 3px 9px; border-radius: 999px; background: #0009; backdrop-filter: blur(6px); display: flex; gap: 6px; align-items: center; }
  .badge i { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); }
  .badge.off i { background: var(--muted); }
  .badge.run i { background: var(--accent); }
  .pchip { position: absolute; left: 10px; top: 36px; z-index: 2; font-size: 10.5px; padding: 2px 8px; border-radius: 999px; color: #fff; background: #000c; backdrop-filter: blur(6px); white-space: nowrap; }
  .glow { position: absolute; inset: 0; opacity: 0; transition: opacity .4s; background: radial-gradient(80% 60% at 50% 100%, color-mix(in srgb, var(--accent) 45%, transparent), transparent 70%); pointer-events: none; z-index: 1; }
  .play { position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%) translateY(12px); opacity: 0; transition: all .35s var(--ease); z-index: 3; padding: 5px 11px; font-size: 12.5px; border-radius: 9px; white-space: nowrap; }
  .g:hover .glow, .g:hover .play, .g:focus-visible .glow, .g:focus-visible .play, .g[aria-disabled='true'] .play, .sel .glow { opacity: 1; }
  .g:hover .play, .g:focus-visible .play, .g[aria-disabled='true'] .play { transform: translateX(-50%); }
  /* sits over the bottom of the (lifted) cover; a sibling, since buttons cannot nest */
  .over { position: absolute; inset: 0 0 auto; aspect-ratio: 2 / 3; display: grid; place-items: end center; padding-bottom: 14px; transform: translateY(-5px) scale(1.02); pointer-events: none; z-index: 4; }
  /* a sibling over the cover's top right corner, like .over */
  .quit { position: absolute; right: 8px; top: 8px; z-index: 5; padding: 5px 8px; font-size: 12px; border-radius: 9px; }
  .install { pointer-events: auto; padding: 5px 11px; font-size: 12.5px; border-radius: 9px; white-space: nowrap; }
  .off :global(.cover) { filter: saturate(.35) brightness(.7); transition: filter .3s; }
  .off:hover :global(.cover), .off:focus-visible :global(.cover), .off.sel :global(.cover) { filter: saturate(.8) brightness(.9); }
  @media (prefers-reduced-motion: reduce) { .g:hover, .g:focus-visible, .g.sel, .over { transform: none; } }
</style>
