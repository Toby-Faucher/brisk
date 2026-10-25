<script lang="ts">
  import type { Game } from '../../state/api';
  import Icon from '../../ui/Icon.svelte';
  import { fmtAgo } from '../home/home';
  import Cover from './Cover.svelte';

  let { game, busy = false, onpick }: { game: Game; busy?: boolean; onpick: (g: Game) => void } = $props();
  const size = $derived(game.sizeBytes ? `${(game.sizeBytes / 1e9).toFixed(game.sizeBytes < 1e10 ? 1 : 0)} GB` : null);
  const sub = $derived(game.installed ? (game.lastPlayed ? `Played ${fmtAgo(game.lastPlayed * 1000)}` : 'Ready to play') : size ? `Not installed · ${size}` : 'Not installed');
</script>

<button class="g" class:off={!game.installed} disabled={busy} onclick={() => onpick(game)} aria-label={`${game.installed ? 'Play' : 'Install'} ${game.name}`}>
  <Cover {game}>
    <span class="badge" class:off={!game.installed}><i></i>{game.installed ? 'Installed' : (size ?? 'Not installed')}</span>
    <span class="glow"></span>
    <span class="btn play"><Icon name={game.installed ? 'play' : 'dl'} size={13} /> {busy ? 'Starting…' : game.installed ? 'Play' : 'Install'}</span>
  </Cover>
  <span class="name">{game.name}</span>
  <span class="sub">{sub}</span>
</button>

<style>
  .g { display: block; width: 100%; text-align: left; padding: 0; border: 0; background: none; cursor: pointer; transition: transform .3s var(--ease); border-radius: 16px; }
  .g:hover, .g:focus-visible { transform: translateY(-5px) scale(1.02); }
  .g:disabled { cursor: progress; }
  .name { display: block; margin-top: 8px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { display: block; color: var(--muted); font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { position: absolute; left: 10px; top: 10px; z-index: 2; font-size: 11px; font-weight: 600; padding: 3px 9px; border-radius: 999px; background: #0009; backdrop-filter: blur(6px); display: flex; gap: 6px; align-items: center; }
  .badge i { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); }
  .badge.off i { background: var(--muted); }
  .glow { position: absolute; inset: 0; opacity: 0; transition: opacity .4s; background: radial-gradient(80% 60% at 50% 100%, color-mix(in srgb, var(--accent) 45%, transparent), transparent 70%); pointer-events: none; z-index: 1; }
  .play { position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%) translateY(12px); opacity: 0; transition: all .35s var(--ease); z-index: 3; padding: 5px 11px; font-size: 12.5px; border-radius: 9px; white-space: nowrap; }
  .g:hover .glow, .g:hover .play, .g:focus-visible .glow, .g:focus-visible .play, .g:disabled .play { opacity: 1; }
  .g:hover .play, .g:focus-visible .play, .g:disabled .play { transform: translateX(-50%); }
  .off :global(.cover) { filter: saturate(.35) brightness(.7); transition: filter .3s; }
  .off:hover :global(.cover), .off:focus-visible :global(.cover) { filter: saturate(.8) brightness(.9); }
  @media (prefers-reduced-motion: reduce) { .g:hover, .g:focus-visible { transform: none; } }
</style>
