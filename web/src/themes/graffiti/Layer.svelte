<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // A stencilled line on the wall, one per page.
  const SAYS: [string, string][] = [
    ['/library', 'Pick a poster'],
    ['/machines', 'Know your block'],
    ['/history', 'Old tags, still up'],
    ['/settings', 'Mind the wet paint'],
    ['/', 'The wall is yours'],
  ];
  const says = $derived(SAYS.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p)))?.[1] ?? 'Keep it moving');

  // Spray dots follow the pointer and fade. One canvas, one loop that sleeps when nothing is left to draw.
  let cv: HTMLCanvasElement | undefined = $state();
  onMount(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches || !cv) return;
    const c = cv.getContext('2d');
    if (!c) return;
    const COLORS = ['#ffe14a', '#4ff0ff', '#ff4fa3', '#8bff4a', '#ff8a2a'];
    type Dot = { x: number; y: number; r: number; born: number; col: string };
    let dots: Dot[] = [];
    let raf = 0;
    let lx = -99, ly = -99, n = 0;
    const size = () => {
      if (!cv) return;
      cv.width = innerWidth;
      cv.height = innerHeight;
    };
    const draw = (t: number) => {
      raf = 0;
      c.clearRect(0, 0, innerWidth, innerHeight);
      dots = dots.filter((d) => t - d.born < 900);
      for (const d of dots) {
        c.globalAlpha = 0.75 * (1 - (t - d.born) / 900);
        c.fillStyle = d.col;
        c.beginPath();
        c.arc(d.x, d.y, d.r, 0, 6.3);
        c.fill();
      }
      if (dots.length) raf = requestAnimationFrame(draw);
    };
    const move = (e: PointerEvent) => {
      if (e.pointerType === 'touch' || Math.hypot(e.clientX - lx, e.clientY - ly) < 14) return;
      lx = e.clientX;
      ly = e.clientY;
      const col = COLORS[Math.floor(n++ / 6) % COLORS.length];
      for (let i = 0; i < 3; i++) {
        dots.push({ x: lx + (Math.random() - 0.5) * 14, y: ly + (Math.random() - 0.5) * 14, r: 1 + Math.random() * 2, born: performance.now(), col });
      }
      raf ||= requestAnimationFrame(draw);
    };
    size();
    addEventListener('resize', size);
    addEventListener('pointermove', move, { passive: true });
    return () => {
      removeEventListener('resize', size);
      removeEventListener('pointermove', move);
      cancelAnimationFrame(raf);
    };
  });
</script>

{#if !stream}
  <div class="gf-wall" aria-hidden="true">
    <i class="gf-stain s1"></i>
    <i class="gf-stain s2"></i>
    <i class="gf-grain"></i>
    <i class="gf-tag">BRISK</i>
  </div>
  <div class="gf-sign" aria-hidden="true">{says}</div>
  <canvas class="gf-spray" bind:this={cv} aria-hidden="true"></canvas>
{/if}

<style>
  /* Scoped here: these exist only while the graffiti style is mounted. */
  .gf-wall { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; }
  .gf-wall i { position: absolute; display: block; }
  .gf-grain { inset: 0; opacity: .5; mix-blend-mode: overlay; background: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='220' height='220'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.85' numOctaves='3' stitchTiles='stitch'/%3E%3CfeColorMatrix values='0 0 0 0 .5 0 0 0 0 .5 0 0 0 0 .5 0 0 0 1.1 -.2'/%3E%3C/filter%3E%3Crect width='220' height='220' filter='url(%23n)'/%3E%3C/svg%3E"); }
  .gf-stain { border-radius: 50%; filter: blur(40px); }
  .s1 { width: 46vmax; height: 30vmax; left: 16vw; top: -8vmax; background: rgb(79 240 255 / .13); }
  .s2 { width: 50vmax; height: 34vmax; right: -12vmax; bottom: -10vmax; background: rgb(255 79 163 / .15); }
  .gf-tag { right: 3vw; bottom: 9vh; font: 400 clamp(90px, 14vw, 220px) / 1 'Permanent Marker', cursive; color: transparent; -webkit-text-stroke: 3px rgb(255 255 255 / .09); rotate: -8deg; user-select: none; }
  /* the stencil sign: spaced caps with a slit through the middle, like a cut template */
  .gf-sign { position: fixed; z-index: 25; left: 14px; bottom: 74px; width: 118px; text-align: center; rotate: -3deg; pointer-events: none; font: 800 11px / 1.5 'Rubik', system-ui, sans-serif; letter-spacing: .2em; text-transform: uppercase; color: rgb(244 240 227 / .9); padding: 8px 6px; border: 2px solid rgb(244 240 227 / .5); mask-image: linear-gradient(#000 0 47%, transparent 47% 50%, #000 50%); }
  .gf-spray { position: fixed; inset: 0; z-index: 90; width: 100vw; height: 100vh; pointer-events: none; }
  @media (max-height: 640px) { .gf-sign { display: none; } }
</style>
