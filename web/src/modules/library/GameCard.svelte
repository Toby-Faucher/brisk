<script lang="ts">
  import type { Game } from '../../state/api';
  import { gameKey, pick } from '../../state/gameId';
  import { prefs, prefsChip } from '../../state/prefs';
  import { gameTags, notes } from '../../state/notes';
  import TagChip from './TagChip.svelte';
  import QuitLabel from '../../ui/QuitLabel.svelte';
  import Tip from '../../ui/Tip.svelte';
  import { fmtAgo } from '../home/home';
  import Cover from './Cover.svelte';
  import { badgeLabel, fmtFortnight, fmtSize, smartTags, updateLabel } from './library';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';

  let { game, onpick, showFortnight = false }: { showFortnight?: boolean; game: Game; onpick: (g: Game) => void } = $props();
  const saved = $derived(pick($prefs, game));
  const tags = $derived(gameTags($notes, game));
  const hasNote = $derived(!!pick($notes.games, game)?.note);
  // Your tags first, then the automatic ones (not Updated, it has its chip); names go into the aria-label, at most 3 symbols are drawn.
  const all = $derived([...tags, ...smartTags(game).filter((t) => t.id !== 'auto-updated')]);
  const tagNames = $derived(all.map((t) => t.name).join(', '));
  const size = $derived(fmtSize(game.sizeBytes));
  const played = $derived(game.lastPlayed ? `Played ${fmtAgo(game.lastPlayed * 1000)}` : 'Ready to play');
  const sub = $derived(showFortnight && game.playtime2wkMin > 0 ? fmtFortnight(game.playtime2wkMin) : game.installed ? played : size ? `Not installed · ${size}` : 'Not installed');
  const upd = $derived(updateLabel(game));
  const label = $derived(
    (game.installed ? (game.running ? `${game.name}, running on the host` : `${game.name}, ${played.toLowerCase()}`) : `${game.name}, not installed${size ? `, ${size}` : ''}`) +
      (upd ? `, ${upd.toLowerCase()}` : '') +
      (game.updated ? ', updated since you played' : '') +
      (saved ? `, saved quality ${prefsChip(saved)}` : '') +
      (game.achTotal ? `, ${game.achUnlocked ?? 0} of ${game.achTotal} achievements` : '') +
      (all.length ? `, tags: ${tagNames}` : '') +
      (hasNote ? ', note' : ''),
  );
</script>

<div class="wrap" data-gid={gameKey(game)}>
  <!-- opens the detail panel (GameDetail); Play and Install live there -->
  <button class="g" class:off={!game.installed} aria-haspopup="dialog" onclick={() => onpick(game)} aria-label={label}>
    <Cover {game}>
      <span class="badge" class:off={!game.installed} class:run={game.running}><i></i>{badgeLabel(game)}</span>
      {#if saved || upd || game.updated || all.length || hasNote}
        <div class="chips">
          {#if saved}<span class="pchip">{prefsChip(saved)}</span><Tip label="Saved quality for this game" />{/if}
          {#if upd}<span class="uchip need" aria-hidden="true">{upd}</span>{/if}
          {#if game.updated}<span class="uchip" aria-hidden="true">Updated</span>{/if}
          {#if all.length || hasNote}
            <!-- in the card's aria-label already; title for the mouse -->
            <span class="tchip" aria-hidden="true" title={[tagNames, hasNote && 'Has a note'].filter(Boolean).join(' · ')}>
              {#each all.slice(0, 3) as t (t.id)}<TagChip color={t.color} symbol={t.symbol} bare size={12} />{/each}
              {#if all.length > 3}<span>+{all.length - 3}</span>{/if}
              {#if hasNote}<svg class="nm" width="12" height="12" viewBox="0 0 16 16" fill="currentColor"><path d="M3 2h10v12H3zm2 3v1.4h6V5zm0 3v1.4h6V8zm0 3v1.4h4V11z" fill-rule="evenodd" /></svg>{/if}
            </span>
          {/if}
        </div>
      {/if}
      <span class="glow"></span>
      <span class="btn play">Details</span>
    </Cover>
    <span class="name">{game.name}</span>
    <span class="sub">{sub}</span>
    {#if game.achTotal}<span class="ach" aria-hidden="true"><i style="width:{((game.achUnlocked ?? 0) / game.achTotal) * 100}%"></i></span>{/if}
  </button>
  {#if game.running}
    {@const q = $quitting.has(gameKey(game))}
    <button class="btn ghost quit" aria-disabled={q} aria-busy={q} onclick={() => askQuit(game)} aria-label={q ? `Quitting ${game.name}` : `Quit ${game.name}`}><QuitLabel busy={q} icon={13} /></button><Tip label="Quit game" />
  {/if}
</div>

<style>
  /* the wrap lifts, so the Quit sibling (outside the card button) moves with the card: same transform, transition and origin */
  .wrap { position: relative; transition: transform var(--spring-soft-dur) var(--spring-soft); }
  .wrap:hover, .wrap:has(:focus-visible) { transform: translateY(-5px) scale(1.02); }
  /* a long name must not widen the card: no min-content from the text, so every cover keeps the slot's width */
  .name, .sub { contain: inline-size; }
  .g { display: block; width: 100%; text-align: left; padding: 0; border: 0; background: none; cursor: pointer; border-radius: 16px; }
  .name { display: block; margin-top: 8px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { display: block; color: var(--muted); font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ach { display: block; height: 3px; margin-top: 5px; border-radius: 2px; background: var(--line); overflow: hidden; }
  .ach i { display: block; height: 100%; background: var(--accent); }
  .badge { position: absolute; left: 10px; top: 10px; z-index: 2; font-size: 11px; font-weight: 600; padding: 3px 9px; border-radius: 999px; background: #0009; backdrop-filter: blur(6px); display: flex; gap: 6px; align-items: center; }
  .badge i { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); }
  .badge.off i { background: var(--muted); }
  .badge.run i { background: var(--accent); }
  .chips { position: absolute; left: 10px; top: 36px; z-index: 2; display: flex; flex-direction: column; align-items: flex-start; gap: 4px; }
  .pchip { font-size: 10.5px; padding: 2px 8px; border-radius: 999px; color: #fff; background: #000c; backdrop-filter: blur(6px); white-space: nowrap; }
  .uchip { font-size: 10.5px; padding: 2px 8px; border-radius: 999px; color: #fff; background: color-mix(in srgb, var(--accent) 80%, #000); white-space: nowrap; }
  .uchip.need { background: #b45309; }
  .tchip { display: flex; align-items: center; gap: 4px; font-size: 10.5px; padding: 3px 7px; border-radius: 999px; color: #fff; background: #000c; backdrop-filter: blur(6px); }
  .glow { position: absolute; inset: 0; opacity: 0; transition: opacity .4s; background: radial-gradient(80% 60% at 50% 100%, color-mix(in srgb, var(--accent) 45%, transparent), transparent 70%); pointer-events: none; z-index: 1; }
  .play { position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%) translateY(12px); opacity: 0; transition: all .35s var(--ease); z-index: 3; padding: 5px 11px; font-size: 12.5px; border-radius: 9px; white-space: nowrap; }
  .g:hover .glow, .g:hover .play, .g:focus-visible .glow, .g:focus-visible .play { opacity: 1; }
  .g:hover .play, .g:focus-visible .play { transform: translateX(-50%); }
  /* a sibling over the cover's top right corner, since buttons cannot nest */
  .quit { position: absolute; right: 8px; top: 8px; z-index: 5; padding: 5px 8px; min-height: 24px; /* WCAG 2.2 target size */ font-size: 12px; border-radius: 9px; }
  .off :global(.cover) { filter: saturate(.35) brightness(.7); transition: filter .3s; }
  .off:hover :global(.cover), .off:focus-visible :global(.cover) { filter: saturate(.8) brightness(.9); }
  @media (prefers-reduced-motion: reduce) { .wrap:hover, .wrap:has(:focus-visible) { transform: none; } }
</style>
