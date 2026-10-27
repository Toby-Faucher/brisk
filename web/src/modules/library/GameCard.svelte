<script lang="ts">
  import type { Game } from '../../state/api';
  import { prefs, prefsChip } from '../../state/prefs';
  import Icon from '../../ui/Icon.svelte';
  import Tip from '../../ui/Tip.svelte';
  import { fmtAgo } from '../home/home';
  import Cover from './Cover.svelte';
  import { badgeLabel, fmtFortnight, fmtSize } from './library';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';

  let { game, onpick, showFortnight = false }: { showFortnight?: boolean; game: Game; onpick: (g: Game) => void } = $props();
  const saved = $derived($prefs[game.appid]);
  const size = $derived(fmtSize(game.sizeBytes));
  const played = $derived(game.lastPlayed ? `Played ${fmtAgo(game.lastPlayed * 1000)}` : 'Ready to play');
  const sub = $derived(showFortnight && game.playtime2wkMin > 0 ? fmtFortnight(game.playtime2wkMin) : game.installed ? played : size ? `Not installed · ${size}` : 'Not installed');
  const label = $derived(
    (game.installed ? (game.running ? `${game.name}, running on the host` : `${game.name}, ${played.toLowerCase()}`) : `${game.name}, not installed${size ? `, ${size}` : ''}`) +
      (saved ? `, saved quality ${prefsChip(saved)}` : ''),
  );
</script>

<div class="wrap" data-appid={game.appid}>
  <!-- opens the detail panel (GameDetail); Play and Install live there -->
  <button class="g" class:off={!game.installed} aria-haspopup="dialog" onclick={() => onpick(game)} aria-label={label}>
    <Cover {game}>
      <span class="badge" class:off={!game.installed} class:run={game.running}><i></i>{badgeLabel(game)}</span>
      {#if saved}<span class="pchip">{prefsChip(saved)}</span><Tip label="Saved quality for this game" />{/if}
      <span class="glow"></span>
      <span class="btn play">Details</span>
    </Cover>
    <span class="name">{game.name}</span>
    <span class="sub">{sub}</span>
  </button>
  {#if game.running}
    {@const q = $quitting.has(game.appid)}
    <button class="btn ghost quit" aria-disabled={q} onclick={() => askQuit(game)} aria-label={q ? `Quitting ${game.name}` : `Quit ${game.name}`}><Icon name="power" size={13} />{q ? ' Quitting…' : ''}</button><Tip label="Quit game" />
  {/if}
</div>

<style>
  .wrap { position: relative; }
  .g { display: block; width: 100%; text-align: left; padding: 0; border: 0; background: none; cursor: pointer; transition: transform var(--spring-soft-dur) var(--spring-soft); border-radius: 16px; }
  .g:hover, .g:focus-visible { transform: translateY(-5px) scale(1.02); }
  .name { display: block; margin-top: 8px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { display: block; color: var(--muted); font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { position: absolute; left: 10px; top: 10px; z-index: 2; font-size: 11px; font-weight: 600; padding: 3px 9px; border-radius: 999px; background: #0009; backdrop-filter: blur(6px); display: flex; gap: 6px; align-items: center; }
  .badge i { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); }
  .badge.off i { background: var(--muted); }
  .badge.run i { background: var(--accent); }
  .pchip { position: absolute; left: 10px; top: 36px; z-index: 2; font-size: 10.5px; padding: 2px 8px; border-radius: 999px; color: #fff; background: #000c; backdrop-filter: blur(6px); white-space: nowrap; }
  .glow { position: absolute; inset: 0; opacity: 0; transition: opacity .4s; background: radial-gradient(80% 60% at 50% 100%, color-mix(in srgb, var(--accent) 45%, transparent), transparent 70%); pointer-events: none; z-index: 1; }
  .play { position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%) translateY(12px); opacity: 0; transition: all .35s var(--ease); z-index: 3; padding: 5px 11px; font-size: 12.5px; border-radius: 9px; white-space: nowrap; }
  .g:hover .glow, .g:hover .play, .g:focus-visible .glow, .g:focus-visible .play { opacity: 1; }
  .g:hover .play, .g:focus-visible .play { transform: translateX(-50%); }
  /* a sibling over the cover's top right corner, since buttons cannot nest */
  .quit { position: absolute; right: 8px; top: 8px; z-index: 5; padding: 5px 8px; min-height: 24px; /* WCAG 2.2 target size */ font-size: 12px; border-radius: 9px; }
  .off :global(.cover) { filter: saturate(.35) brightness(.7); transition: filter .3s; }
  .off:hover :global(.cover), .off:focus-visible :global(.cover) { filter: saturate(.8) brightness(.9); }
  @media (prefers-reduced-motion: reduce) { .g:hover, .g:focus-visible { transform: none; } }
</style>
