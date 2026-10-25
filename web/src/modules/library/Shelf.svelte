<script lang="ts">
  import type { Game } from '../../state/api';
  import GameCard from './GameCard.svelte';
  import { visibleRange } from './library';

  let { title, games, busy, onpick }: { title: string; games: Game[]; busy: number | null; onpick: (g: Game) => void } = $props();

  const CARD = 138;
  const GAP = 14;
  const PAGE = 60;
  let left = $state(0);
  let width = $state(1200);
  let grid = $state(false);
  let shown = $state(PAGE);
  const r = $derived(visibleRange(left, width, CARD + GAP, games.length));

  // The Shell remounts on navigation, so the observer and listener go with the node.
  const track = (node: HTMLElement) => {
    const ro = new ResizeObserver(() => (width = node.clientWidth));
    const onScroll = () => (left = node.scrollLeft);
    ro.observe(node);
    node.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      ro.disconnect();
      node.removeEventListener('scroll', onScroll);
    };
  };
  const toggle = () => {
    grid = !grid;
    shown = PAGE;
    left = 0;
  };
</script>

<section class="shelf">
  <h4>
    <span>{title} <span class="n">{games.length}</span></span>
    {#if games.length > 6}<button class="more" onclick={toggle}>{grid ? 'Show less' : 'Show all'}</button>{/if}
  </h4>
  {#if grid}
    <div class="grid">
      {#each games.slice(0, shown) as g (g.appid)}<GameCard game={g} busy={busy === g.appid} {onpick} />{/each}
    </div>
    {#if shown < games.length}<button class="btn ghost page" onclick={() => (shown += PAGE)}>Show {Math.min(PAGE, games.length - shown)} more</button>{/if}
  {:else}
    <!-- Only the cards near the viewport are mounted; the track keeps the full scroll width. -->
    <div class="row" {@attach track}>
      <div class="track" style:width="{games.length * (CARD + GAP) - GAP}px" style:padding-left="{r.start * (CARD + GAP)}px">
        {#each games.slice(r.start, r.end) as g (g.appid)}
          <div class="slot"><GameCard game={g} busy={busy === g.appid} {onpick} /></div>
        {/each}
      </div>
    </div>
  {/if}
</section>

<style>
  .shelf { margin-top: 20px; }
  h4 { margin: 0 0 10px; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; display: flex; justify-content: space-between; align-items: center; }
  .n { margin-left: 6px; font-family: var(--mono); color: var(--dim); }
  .more { border: 0; background: none; color: var(--muted); cursor: pointer; font-size: 12.5px; font-weight: 500; letter-spacing: 0; text-transform: none; padding: 2px 6px; border-radius: 8px; }
  .more:hover { color: var(--fg); }
  .row { overflow-x: auto; padding: 8px 0 8px; margin-top: -8px; scrollbar-width: thin; }
  .track { display: flex; gap: 14px; }
  .slot { flex: 0 0 138px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 16px; }
  .page { margin: 16px auto 0; display: flex; }
</style>
