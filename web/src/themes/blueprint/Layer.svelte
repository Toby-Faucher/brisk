<script lang="ts">
  // Drafting sheet over the page: border frame with zone marks, a title block, a red-pencil note, a faint crosshair
  // and live dimension numbers on cards. Never on /stream, never catches a click, no per-frame layout reads.
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const on = $derived(!/^\/stream/.test(path));

  const SHEETS: Record<string, [string, string, string]> = {
    '': ['01', 'HOME', 'is the host awake? check the kettle'],
    library: ['02', 'LIBRARY', 'too many games. add shelves'],
    machines: ['03', 'MACHINES', 'who unplugged the big one?'],
    history: ['04', 'HISTORY', '3 am again? we will not discuss it'],
    settings: ['05', 'SETTINGS', 'do not touch (touch it anyway)'],
  };
  const sheet = $derived(SHEETS[path.split('/')[1] ?? ''] ?? ['06', 'MISC', 'not on the drawing list']);

  let cross: HTMLElement | undefined = $state();
  let xy: HTMLElement | undefined = $state();
  let moved = $state(false);

  onMount(() => {
    // cursor readout: one rAF per frame at most, writes a transform and one text node
    let px = 0, py = 0, queued = false;
    const move = (e: PointerEvent) => {
      moved = true;
      px = Math.round(e.clientX);
      py = Math.round(e.clientY);
      if (queued) return;
      queued = true;
      requestAnimationFrame(() => {
        queued = false;
        if (cross) cross.style.transform = `translate(${px}px, ${py}px)`;
        if (xy) xy.textContent = `X ${String(px).padStart(4, '0')}  Y ${String(py).padStart(4, '0')}`;
      });
    };
    addEventListener('pointermove', move, { passive: true });

    // real widths on every drawing card: ResizeObserver hands us the size, so nothing is read from layout
    const SEL = "div.card[role='group'], .hero, .chart";
    const ro = new ResizeObserver((es) => {
      for (const e of es) (e.target as HTMLElement).style.setProperty('--dw', String(Math.round(e.borderBoxSize?.[0]?.inlineSize ?? e.contentRect.width)));
    });
    const seen = new WeakSet<Element>();
    const scan = () => {
      for (const el of document.querySelectorAll(SEL)) if (!seen.has(el)) (seen.add(el), ro.observe(el));
    };
    let q = false;
    const mo = new MutationObserver(() => {
      if (q) return;
      q = true;
      requestAnimationFrame(() => ((q = false), scan()));
    });
    scan();
    mo.observe(document.querySelector('main') ?? document.body, { childList: true, subtree: true });
    return () => (removeEventListener('pointermove', move), ro.disconnect(), mo.disconnect());
  });
</script>

{#if on}
  <div class="sheet" aria-hidden="true">
    <div class="frame"></div>
    <div class="zones zt">{#each [1, 2, 3, 4, 5, 6, 7, 8] as n}<i>{n}</i>{/each}</div>
    <div class="zones zl">{#each ['A', 'B', 'C', 'D', 'E', 'F'] as n}<i>{n}</i>{/each}</div>
    <div class="cross" class:on={moved} bind:this={cross}><b class="h"></b><b class="v"></b></div>
    {#key path}
      <div class="note"><span>{sheet[2]}</span></div>
      <div class="tb">
        <div class="r1"><span class="co">BRISK / STREAM WORKS</span><span>SHEET {sheet[0]}/06</span></div>
        <div class="ti"><small>DRAWING TITLE</small><b>{sheet[1]}</b></div>
        <div class="r3"><span>REV C</span><span>SCALE 1:1</span><span>DRN T.F.</span><span>CHKD NOBODY</span></div>
        <div class="xy" bind:this={xy}>X 0000  Y 0000</div>
      </div>
    {/key}
  </div>
{/if}

<style>
  .sheet { position: fixed; inset: 0; z-index: 30; pointer-events: none; overflow: hidden; font-family: 'Share Tech Mono', ui-monospace, monospace; color: #dcecff; }
  .frame { position: absolute; inset: 16px; border: 1px solid rgba(220, 236, 255, 0.75); box-shadow: 0 0 0 4px rgba(220, 236, 255, 0.05); animation: wipe 1.1s cubic-bezier(0.6, 0, 0.2, 1) backwards; }
  .zones { position: absolute; display: flex; justify-content: space-around; font-size: 10px; color: rgba(220, 236, 255, 0.8); font-style: normal; }
  .zones i { font-style: normal; }
  .zones.zt { left: 16px; right: 16px; top: 2px; line-height: 14px; }
  .zones.zl { top: 16px; bottom: 16px; left: 0; width: 16px; flex-direction: column; align-items: center; }
  .cross { position: absolute; left: 0; top: 0; will-change: transform; opacity: 0; }
  .cross.on { opacity: 1; }
  .cross b { position: absolute; background: rgba(160, 210, 255, 0.16); }
  .cross .h { left: -3000px; width: 6000px; height: 1px; }
  .cross .v { top: -3000px; height: 6000px; width: 1px; }
  .note { position: absolute; left: 30px; bottom: 96px; width: 196px; font-family: 'Architects Daughter', cursive; font-size: 19px; color: #ff9d94; rotate: -4deg; transform-origin: left bottom; line-height: 1.15; text-shadow: 0 0 1px rgba(255, 157, 148, 0.4); }
  .note span { display: block; animation: scribble 1.6s steps(28, end) 0.9s backwards; }
  .note::after { content: ''; display: block; height: 7px; margin-top: 1px; background: radial-gradient(7px 4px at 7px 4px, transparent 55%, #ff9d94 56% 70%, transparent 71%) 0 0 / 14px 8px repeat-x; opacity: 0.8; animation: scribble 0.8s linear 2.4s backwards; }
  .tb { position: absolute; right: 16px; bottom: 16px; width: 276px; border: 1px solid rgba(220, 236, 255, 0.85); background: #082a72; font-size: 11px; letter-spacing: 0.06em; animation: slide 0.8s cubic-bezier(0.2, 0.8, 0.2, 1) 0.5s backwards; }
  .tb > div { padding: 3px 8px; border-bottom: 1px solid rgba(220, 236, 255, 0.45); }
  .tb > div:last-child { border-bottom: 0; }
  .r1, .r3 { display: flex; justify-content: space-between; gap: 6px; color: #b9d3f4; }
  .co { color: #fff; }
  .ti small { display: block; font-size: 9px; color: #a3c3ec; }
  .ti b { display: block; font-size: 22px; font-weight: 400; letter-spacing: 0.22em; color: #fff; line-height: 1.1; }
  .xy { color: #8fe3ff; font-variant-numeric: tabular-nums; white-space: pre; }
  @keyframes wipe { from { clip-path: inset(0 100% 100% 0); } to { clip-path: inset(0 0 0 0); } }
  @keyframes slide { from { clip-path: inset(0 0 0 100%); } }
  @keyframes scribble { from { clip-path: inset(0 100% 0 0); } to { clip-path: inset(0); } }
  @media (max-width: 900px), (max-height: 560px) { .note, .zones, .cross { display: none; } .tb { width: 200px; } }
  @media (prefers-reduced-motion: reduce) { .cross { display: none; } }
</style>
