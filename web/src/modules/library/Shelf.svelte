<script lang="ts">
  import type { Game } from '../../state/api';
  import GameCard from './GameCard.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { visibleRange, wheelToX } from './library';

  let { title, games, busy, selected, onpick, oninstall }: { title: string; games: Game[]; busy: number | null; selected: number | null; onpick: (g: Game) => void; oninstall: (g: Game) => void } = $props();

  const CARD = 138;
  const GAP = 14;
  const PAGE = 60;
  let left = $state(0);
  let width = $state(1200);
  let grid = $state(false);
  let shown = $state(PAGE);
  let row: HTMLElement | undefined = $state();
  const r = $derived(visibleRange(left, width, CARD + GAP, games.length));
  const full = $derived(games.length * (CARD + GAP) - GAP);
  const atStart = $derived(left <= 0);
  const atEnd = $derived(left + width >= full - 1);

  // The Shell remounts on navigation, so the observer and listener go with the node.
  const track = (node: HTMLElement) => {
    const ro = new ResizeObserver(() => (width = node.clientWidth));
    const onScroll = () => (left = node.scrollLeft);
    // a vertical wheel over the shelf moves it sideways until it hits an edge, then the page scrolls as usual
    const onWheel = (e: WheelEvent) => {
      const dx = wheelToX(e.deltaX, e.deltaY, node.scrollLeft, node.clientWidth, node.scrollWidth);
      if (dx == null) return;
      e.preventDefault();
      node.scrollLeft += dx;
    };
    ro.observe(node);
    node.addEventListener('scroll', onScroll, { passive: true });
    node.addEventListener('wheel', onWheel, { passive: false });
    row = node;
    return () => {
      ro.disconnect();
      node.removeEventListener('scroll', onScroll);
      node.removeEventListener('wheel', onWheel);
      row = undefined;
    };
  };
  const page = (dir: number) => row?.scrollBy({ left: dir * Math.max(CARD + GAP, row.clientWidth - 2 * (CARD + GAP)), behavior: 'smooth' });
  const toggle = () => {
    grid = !grid;
    shown = PAGE;
    left = 0;
  };
</script>

<section class="shelf">
  <h4 class="reveal">
    <span>{title} <span class="n">{games.length}</span></span>
    <span class="acts">
      {#if !grid && !(atStart && atEnd)}
        <button class="chev" aria-label="Scroll {title} left" aria-disabled={atStart} onclick={() => page(-1)}><Icon name="chev" size={16} /></button>
        <button class="chev r" aria-label="Scroll {title} right" aria-disabled={atEnd} onclick={() => page(1)}><Icon name="chev" size={16} /></button>
      {/if}
      {#if games.length > 6}<button class="more" aria-expanded={grid} onclick={toggle}>{grid ? 'Show less' : 'Show all'}</button>{/if}
    </span>
  </h4>
  {#if grid}
    <div class="grid">
      {#each games.slice(0, shown) as g (g.appid)}<GameCard game={g} busy={busy === g.appid} selected={selected === g.appid} {onpick} {oninstall} />{/each}
    </div>
    {#if shown < games.length}<button class="btn ghost page" onclick={() => (shown += PAGE)}>Show {Math.min(PAGE, games.length - shown)} more</button>{/if}
  {:else}
    <!-- Only the cards near the viewport are mounted; the track keeps the full scroll width. -->
    <div class="row" {@attach track}>
      <div class="track" style:width="{full}px" style:padding-left="{r.start * (CARD + GAP)}px">
        {#each games.slice(r.start, r.end) as g, i (g.appid)}
          <div class="slot" class:enter={r.start === 0 && i < 8}><GameCard game={g} busy={busy === g.appid} selected={selected === g.appid} {onpick} {oninstall} /></div>
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
  .more:hover, .chev:hover { color: var(--fg); }
  .acts { display: flex; gap: 4px; align-items: center; }
  .chev { border: 1px solid var(--line); background: var(--panel); color: var(--muted); cursor: pointer; width: 26px; height: 26px; border-radius: 8px; display: grid; place-items: center; padding: 0; }
  .chev:not(.r) :global(svg) { transform: scaleX(-1); }
  .chev[aria-disabled='true'] { opacity: .35; cursor: default; }
  .row { overflow-x: auto; overscroll-behavior-x: contain; padding: 8px 0 8px; margin-top: -8px; }
  .track { display: flex; gap: 14px; }
  .slot { flex: 0 0 138px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 16px; }
  .page { margin: 16px auto 0; display: flex; }
</style>
