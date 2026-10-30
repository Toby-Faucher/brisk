<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // HUD: score and coins go up on every page change and every button press. Time counts down and wraps.
  let score = $state(0);
  let coins = $state(0);
  let time = $state(400);
  let pops = $state<{ id: number; x: number; y: number }[]>([]);
  let seen = 0;
  let id = 0;
  const WORLDS: Record<string, string> = { '': '1-1', library: '1-2', machines: '2-1', history: '3-1', settings: '4-1', stream: '8-4' };
  const world = $derived(WORLDS[path.replace(/^\//, '').split('/')[0]] ?? '1-1');

  const coin = (x: number, y: number) => {
    score += 200;
    coins = (coins + 1) % 100;
    const p = { id: id++, x, y };
    pops = [...pops.slice(-5), p];
    setTimeout(() => (pops = pops.filter((q) => q.id !== p.id)), 700);
  };
  $effect(() => {
    void path;
    if (seen++ === 0) return;
    score += 100;
  });

  onMount(() => {
    const tick = setInterval(() => (time = time > 0 ? time - 1 : 400), 1000);
    const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
    // Pressing any button bumps it and pops a coin out of it, like hitting a block from below.
    const press = (e: PointerEvent) => {
      const b = (e.target as Element | null)?.closest?.('.btn, .chip, .st .tab, .side a.nav:not(.soon)');
      if (!(b instanceof HTMLElement)) return;
      const r = b.getBoundingClientRect();
      if (!reduce) {
        b.classList.remove('pf-bump');
        void b.offsetWidth;
        b.classList.add('pf-bump');
        setTimeout(() => b.classList.remove('pf-bump'), 300);
        coin(r.left + r.width / 2, r.top);
      } else score += 200;
    };
    addEventListener('pointerdown', press, { passive: true });
    return () => {
      clearInterval(tick);
      removeEventListener('pointerdown', press);
    };
  });
  const pad = (n: number, w: number) => String(n).padStart(w, '0');
</script>

{#if !stream}
  <div class="pf-sky" aria-hidden="true">
    <i class="pf-cloud c1"></i><i class="pf-cloud c2"></i><i class="pf-cloud c3"></i><i class="pf-cloud c4"></i>
    <i class="pf-hills h2"></i><i class="pf-hills h1"></i>
    <i class="pf-flag"><u></u></i><i class="pf-bush b1"></i><i class="pf-bush b2"></i>
  </div>
  <div class="pf-hud" aria-hidden="true">
    <span class="pf-cell"><small>Score</small><b>{pad(score, 6)}</b></span>
    <span class="pf-cell"><i class="pf-coin"></i><small>x</small><b>{pad(coins, 2)}</b></span>
    <span class="pf-cell"><small>World</small><b>{world}</b></span>
    <span class="pf-cell"><small>Time</small><b>{pad(time, 3)}</b></span>
    <span class="pf-cell stars"><i class="pf-star"></i><i class="pf-star"></i><i class="pf-star dim"></i></span>
  </div>
  <div class="pf-ground"></div>
  {#each pops as p (p.id)}
    <i class="pf-pop" style:left="{p.x}px" style:top="{p.y}px"></i>
  {/each}
{/if}

<style>
  .pf-sky { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: linear-gradient(#4aa8ff, #9ad6ff 70%, #c9ecff); }
  .pf-sky i { position: absolute; display: block; }
  .pf-cloud { height: 34px; width: 120px; border-radius: 40px; background: #fff; box-shadow: 40px -16px 0 -2px #fff, 76px 0 0 -4px #fff, 0 6px 0 #dff1ff; animation: pf-drift 120s linear infinite; }
  .c1 { top: 12%; left: -10%; }
  .c2 { top: 28%; left: 30%; scale: .8; animation-duration: 160s; animation-delay: -60s; }
  .c3 { top: 8%; left: 62%; scale: 1.2; animation-duration: 200s; animation-delay: -120s; }
  .c4 { top: 40%; left: 85%; scale: .65; animation-duration: 140s; animation-delay: -30s; }
  .pf-hills { left: 0; right: 0; bottom: 36px; height: 150px; background:
    radial-gradient(ellipse 160px 110px at 12% 100%, #3fae4a 98%, transparent 100%),
    radial-gradient(ellipse 240px 150px at 38% 100%, #3fae4a 98%, transparent 100%),
    radial-gradient(ellipse 180px 120px at 70% 100%, #3fae4a 98%, transparent 100%),
    radial-gradient(ellipse 140px 90px at 92% 100%, #3fae4a 98%, transparent 100%); }
  .h2 { height: 210px; opacity: .55; filter: hue-rotate(10deg) brightness(1.15); transform: translateX(8%); }
  .pf-bush { bottom: 36px; width: 90px; height: 34px; border-radius: 40px 40px 0 0; background: #2e8f3c; box-shadow: 34px 8px 0 -4px #2e8f3c, -30px 10px 0 -6px #2e8f3c; }
  .b1 { left: 56%; }
  .b2 { left: 8%; scale: .8; }
  @keyframes pf-drift { to { transform: translateX(130vw); } }

  .pf-hud { position: fixed; inset: 0 0 auto 0; height: 40px; z-index: 30; pointer-events: none; display: flex; gap: 8px; align-items: center; justify-content: center; padding: 0 12px; background: #1b1b3a; border-bottom: 4px solid #000; font-family: 'Lilita One', sans-serif; color: #fff; letter-spacing: .06em; }
  .pf-cell { display: flex; align-items: baseline; gap: 6px; padding: 2px 12px; font-size: 16px; }
  .pf-cell small { font-size: 12px; color: #ffd84a; text-transform: uppercase; }
  .pf-cell b { font-weight: 400; font-variant-numeric: tabular-nums; }
  .pf-coin { align-self: center; width: 12px; height: 18px; border-radius: 50%; background: #ffcf2e; box-shadow: inset -3px 0 #c98a00; animation: pf-spin 1.1s steps(6) infinite; }
  .stars { gap: 4px; align-items: center; }
  .pf-star { width: 18px; height: 18px; background: #ffd84a; clip-path: polygon(50% 0, 62% 36%, 100% 38%, 70% 60%, 80% 100%, 50% 76%, 20% 100%, 30% 60%, 0 38%, 38% 36%); }
  .pf-star.dim { background: #5a5a7a; }
  @keyframes pf-spin { 0%, 100% { scale: 1 1; } 50% { scale: .15 1; } }

  .pf-ground { position: fixed; inset: auto 0 0 0; height: 36px; z-index: 30; pointer-events: none; background:
    linear-gradient(#5fd35f 0 8px, #2d8a2d 8px 11px, transparent 11px),
    repeating-linear-gradient(90deg, transparent 0 31px, #5a2e0a 31px 33px) 0 11px / 100% 100% no-repeat,
    #c8742a; border-top: 3px solid #000; }
  .pf-flag { right: 56px; bottom: 36px; width: 6px; height: 150px; background: #e8f0e8; border: 2px solid #000; border-radius: 3px; }
  .pf-flag::before { content: ''; position: absolute; top: -12px; left: -5px; width: 12px; height: 12px; border-radius: 50%; background: #ffcf2e; border: 2px solid #000; }
  .pf-flag u { position: absolute; top: 8px; right: 4px; width: 38px; height: 26px; background: #e63946; clip-path: polygon(0 50%, 100% 0, 100% 100%); transform-origin: right; animation: pf-wave 2s ease-in-out infinite; }
  @keyframes pf-wave { 50% { transform: skewY(6deg) scaleX(.9); } }

  .pf-pop { position: fixed; z-index: 40; width: 14px; height: 22px; margin: -22px 0 0 -7px; border-radius: 50%; background: #ffcf2e; box-shadow: inset -3px 0 #c98a00, 0 0 0 2px #000; pointer-events: none; animation: pf-pop .65s ease-out forwards; }
  @keyframes pf-pop { 0% { transform: translateY(0); } 40% { transform: translateY(-46px) scaleX(.3); } 70% { transform: translateY(-40px); } 100% { transform: translateY(-18px); opacity: 0; } }

  @media (max-width: 800px) { .pf-cell.stars { display: none; } .pf-cell { padding: 2px 6px; } }
  @media (prefers-reduced-motion: reduce) {
    .pf-cloud, .pf-coin, .pf-flag u { animation: none; }
    .pf-pop { display: none; }
  }
</style>
