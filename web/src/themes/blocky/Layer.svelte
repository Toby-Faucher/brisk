<script lang="ts">
  // Scenery for the voxel style: a sky that follows the clock (square sun by day, moon and stars by night, blocky clouds),
  // a grass line, an XP bar over the hotbar (level = hours played), and the held-item name that pops up on a page change.
  // All decoration: aria-hidden, no pointer events. Not mounted on /stream.
  import { onMount } from 'svelte';
  import { games } from '../../state/games';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));
  const TITLES: [string, string][] = [
    ['/library', 'Library'],
    ['/machines', 'Machines'],
    ['/history', 'History'],
    ['/settings', 'Settings'],
    ['/', 'Home'],
  ];
  const title = $derived(TITLES.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p)))?.[1] ?? 'Brisk');

  let now = $state(new Date());
  onMount(() => {
    const t = setInterval(() => (now = new Date()), 60_000);
    return () => clearInterval(t);
  });
  const hour = $derived(now.getHours() + now.getMinutes() / 60);
  const day = $derived(hour >= 6 && hour < 18);
  // Sun and moon walk an arc across the top: p runs 0 to 1 over the twelve hours each one is up.
  const p = $derived((((hour - 6 + 24) % 24) % 12) / 12);
  const left = $derived(`${6 + p * 84}%`);
  const top = $derived(`${52 - Math.sin(p * Math.PI) * 44}px`);

  const mins = $derived(($games?.games ?? []).reduce((n, g) => n + (g.playtimeMin ?? 0), 0));
  const level = $derived(Math.floor(mins / 600));
  const fill = $derived(((mins % 600) / 600) * 100);

  // The held-item name shows for a moment after the page changes (and once on load).
  let shown = $state(true);
  $effect(() => {
    void path;
    shown = false;
    const raf = requestAnimationFrame(() => (shown = true));
    const t = setTimeout(() => (shown = false), 2200);
    return () => (cancelAnimationFrame(raf), clearTimeout(t));
  });
</script>

{#if !stream}
  <div class="bk-layer" aria-hidden="true">
    <div class="bk-sky" class:night={!day}>
      {#if !day}
        <i class="star" style="left:8%;top:18px"></i>
        <i class="star" style="left:21%;top:62px"></i>
        <i class="star" style="left:37%;top:30px"></i>
        <i class="star" style="left:55%;top:70px"></i>
        <i class="star" style="left:68%;top:22px"></i>
        <i class="star" style="left:83%;top:58px"></i>
        <i class="star" style="left:94%;top:34px"></i>
      {/if}
      <div class="orb" class:moon={!day} style="left:{left};top:{top}"></div>
      <div class="cloud c1"></div>
      <div class="cloud c2"></div>
      <div class="cloud c3"></div>
      <div class="grass"></div>
    </div>
    <div class="bk-hud">
      <div class="item" class:on={shown}>{title}</div>
      <div class="lvl">{level}</div>
      <div class="xp"><i style="width:{fill}%"></i></div>
    </div>
  </div>
{/if}

<style>
  .bk-layer {
    pointer-events: none;
  }
  .bk-sky {
    position: fixed;
    inset: 0 0 auto 0;
    height: 124px;
    z-index: -1;
    overflow: hidden;
    background: linear-gradient(#5f9df0, #9cc8ff);
    image-rendering: pixelated;
  }
  .bk-sky.night {
    background: linear-gradient(#070b24, #1a2a5c);
  }
  .grass {
    position: absolute;
    inset: auto 0 0 0;
    height: 32px;
    background: var(--bk-grass) 0 0 / 32px 32px repeat-x;
    box-shadow: 0 3px 0 #000;
  }
  .orb {
    position: absolute;
    width: 44px;
    height: 44px;
    background: #ffe94a;
    box-shadow:
      inset -6px -6px 0 #f2b81c,
      inset 6px 6px 0 #fff7a8,
      0 0 0 4px #ffe94a66;
  }
  .orb.moon {
    background: #e6e9f0;
    box-shadow:
      inset -6px -6px 0 #b4bacb,
      inset 6px 6px 0 #fff,
      0 0 0 4px #e6e9f033;
  }
  .star {
    position: absolute;
    width: 4px;
    height: 4px;
    background: #fff;
    opacity: 0.8;
  }
  /* a cloud is a few white blocks: the body, a bump on top (::before) and a smaller one (::after) */
  .cloud {
    position: absolute;
    width: 96px;
    height: 24px;
    background: #fff;
    box-shadow: inset 0 -6px 0 #dde6f5;
    animation: drift 120s linear infinite;
  }
  .night .cloud {
    background: #3b4872;
    box-shadow: inset 0 -6px 0 #2b3557;
  }
  .cloud::before,
  .cloud::after {
    content: '';
    position: absolute;
    background: inherit;
  }
  .cloud::before {
    width: 48px;
    height: 18px;
    left: 18px;
    top: -18px;
  }
  .cloud::after {
    width: 24px;
    height: 12px;
    left: 60px;
    top: -12px;
  }
  .c1 {
    top: 30px;
    left: -120px;
  }
  .c2 {
    top: 66px;
    left: -120px;
    animation-duration: 170s;
    animation-delay: -70s;
    scale: 0.75;
  }
  .c3 {
    top: 14px;
    left: -120px;
    animation-duration: 210s;
    animation-delay: -150s;
    scale: 0.6;
  }
  @keyframes drift {
    to {
      translate: calc(100vw + 260px) 0;
    }
  }

  .bk-hud {
    position: fixed;
    left: 50%;
    bottom: 110px;
    translate: -50% 0;
    width: 420px;
    max-width: calc(100vw - 24px);
    z-index: 29;
    display: grid;
    justify-items: center;
    gap: 3px;
  }
  .item {
    font: 700 13px var(--font);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: #fff;
    text-shadow: 2px 2px 0 #000;
    margin-bottom: 8px;
    opacity: 0;
    transition: opacity 0.4s steps(4);
  }
  .item.on {
    opacity: 1;
    transition-duration: 0s;
  }
  .lvl {
    font: 700 16px var(--font);
    color: #7bd33a;
    text-shadow:
      2px 0 #000,
      -2px 0 #000,
      0 2px #000,
      0 -2px #000;
  }
  .xp {
    width: 100%;
    height: 10px;
    background: #1b1b1b;
    box-shadow:
      0 0 0 3px #000,
      inset 0 0 0 2px #555;
  }
  .xp i {
    display: block;
    height: 100%;
    background: repeating-linear-gradient(90deg, #7bd33a 0 6px, #5aa626 6px 8px);
  }
  @media (prefers-reduced-motion: reduce) {
    .cloud {
      animation: none;
      left: 18%;
    }
    .c2 {
      left: 58%;
    }
    .c3 {
      left: 82%;
    }
    .item {
      transition: none;
    }
  }
  @media (max-width: 760px) {
    .bk-hud {
      bottom: 98px;
    }
  }
</style>
