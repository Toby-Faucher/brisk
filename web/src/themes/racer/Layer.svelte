<script lang="ts" module>
  // Shell (and so this layer) remounts on every page change; the lap count lives here.
  let laps = 0;
  let lapStart = Date.now();
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // Start lights: red, yellow, green run on each page change, then go dark. Lap counts page changes.
  let lit = $state(0);
  let lap = $state(1);
  let lapMs = $state(0);
  let speed = $state(0);
  let rpm = $state(0);
  let boost = 0;
  let timers: ReturnType<typeof setTimeout>[] = [];
  const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const CALLS = ['Lights out', 'Clean start', 'Green flag', 'Pit lane clear', 'Box box. Kidding', 'Full send', 'Slipstream on'];
  let call = $state(CALLS[0]);

  $effect(() => {
    void path;
    if (stream) return;
    laps += 1;
    lap = laps;
    lapStart = Date.now();
    call = CALLS[laps % CALLS.length];
    boost = 1;
    lit = 1;
    timers.forEach(clearTimeout);
    const step = reduced ? 0 : 320;
    timers = [setTimeout(() => (lit = 2), step), setTimeout(() => (lit = 3), step * 2), setTimeout(() => (lit = 0), step * 2 + 1400)];
    return () => timers.forEach(clearTimeout);
  });

  // Speed follows scrolling and gets a kick on every page change. One rAF loop that sleeps once everything settled.
  onMount(() => {
    if (reduced) return;
    let raf = 0, last = 0, ly = scrollY, v = 0, lapTick = 0;
    const tick = (t: number) => {
      raf = 0;
      const dt = Math.min(0.05, (t - (last || t)) / 1000);
      last = t;
      const target = Math.min(1, v + boost);
      rpm += (target - rpm) * Math.min(1, dt * 7);
      v *= Math.pow(0.04, dt);
      boost *= Math.pow(0.25, dt);
      const idle = 0.03 + Math.sin(t / 380) * 0.012;
      const r = Math.min(1, rpm + idle);
      speed = Math.round(r * 214);
      document.documentElement.style.setProperty('--rev', r.toFixed(3));
      if (t - lapTick > 100) (lapTick = t), (lapMs = Date.now() - lapStart);
      raf = requestAnimationFrame(tick);
    };
    const kick = () => (raf ||= requestAnimationFrame(tick));
    const onScroll = () => {
      v = Math.min(1, v + Math.abs(scrollY - ly) / 260);
      ly = scrollY;
    };
    addEventListener('scroll', onScroll, { passive: true, capture: true });
    kick();
    return () => {
      removeEventListener('scroll', onScroll, true);
      cancelAnimationFrame(raf);
      document.documentElement.style.removeProperty('--rev');
    };
  });

  // Lap clock when motion is reduced: once a second is enough.
  onMount(() => {
    if (!reduced) return;
    const t = setInterval(() => (lapMs = Date.now() - lapStart), 1000);
    return () => clearInterval(t);
  });

  const fmt = (ms: number) => {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}.${Math.floor((ms % 1000) / 100)}`;
  };
</script>

{#if !stream}
  <div class="rc-lines" aria-hidden="true"><i class="l1"></i><i class="l2"></i><i class="l3"></i></div>
  <div class="rc-bar" aria-hidden="true">
    <div class="rc-flag"></div>
    <div class="rc-lap"><span>Lap</span><b>{String(lap).padStart(2, '0')}</b></div>
    <div class="rc-time"><span>Time</span><b>{fmt(lapMs)}</b></div>
    <div class="rc-call">{call}</div>
    <div class="rc-lights">
      <i class:on={lit >= 1} class="r"></i><i class:on={lit >= 2} class="y"></i><i class:on={lit >= 3} class="g"></i>
    </div>
    <div class="rc-gauge">
      <svg viewBox="0 0 100 62" width="86" height="54">
        <path d="M10 56 A40 40 0 1 1 90 56" pathLength="100" class="trk" />
        <path d="M10 56 A40 40 0 1 1 90 56" pathLength="100" class="red" />
        <path d="M10 56 A40 40 0 1 1 90 56" pathLength="100" class="val" style:stroke-dasharray="{Math.min(1, Math.max(0, speed / 214)) * 100} 100" />
      </svg>
      <div class="rc-spd"><b>{String(speed).padStart(3, '0')}</b><span>km/h</span></div>
    </div>
  </div>
{/if}

<style>
  .rc-lines { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; }
  .rc-lines i { position: absolute; left: 0; display: block; height: 100%; opacity: .9; animation: rc-run var(--t) linear infinite; }
  .l1 { --w: 300px; --t: 7s; width: calc(100% + 300px); background: linear-gradient(90deg, transparent, rgb(255 255 255 / .1) 70%, transparent) 40px 18% / 300px 2px no-repeat, linear-gradient(90deg, transparent, rgb(255 255 255 / .08) 70%, transparent) 150px 71% / 300px 2px no-repeat; }
  .l2 { --w: 500px; --t: 5s; width: calc(100% + 500px); background: linear-gradient(90deg, transparent, rgb(255 210 31 / .14) 80%, transparent) 120px 33% / 500px 3px no-repeat, linear-gradient(90deg, transparent, rgb(255 255 255 / .09) 80%, transparent) 300px 88% / 500px 2px no-repeat; }
  .l3 { --w: 250px; --t: 3.4s; width: calc(100% + 250px); background: linear-gradient(90deg, transparent, rgb(255 255 255 / .07) 60%, transparent) 20px 52% / 250px 1px no-repeat, linear-gradient(90deg, transparent, rgb(229 50 45 / .16) 60%, transparent) 90px 9% / 250px 2px no-repeat; }
  /* each strip holds one streak per 100vh tile, so tile them across */
  .rc-lines i { background-repeat: repeat-x; }
  .rc-bar { position: fixed; z-index: 25; top: 0; left: var(--side-w, 230px); right: 0; height: 56px; display: flex; align-items: center; gap: 18px; padding: 0 18px 0 0; pointer-events: none; background: #1b1d21; border-bottom: 8px solid transparent; border-image: repeating-linear-gradient(90deg, #e5322d 0 22px, #fff 22px 44px) 0 0 8 0 / 0 0 8px 0 stretch; color: #fff; font: 400 14px/1 'Russo One', system-ui, sans-serif; font-style: italic; }
  .rc-flag { align-self: stretch; width: 56px; flex: 0 0 auto; margin-right: 4px; background: conic-gradient(#fff 25%, #111 0 50%, #fff 0 75%, #111 0) 0 0 / 14px 14px; }
  .rc-lap, .rc-time { display: flex; flex-direction: column; gap: 3px; }
  .rc-lap span, .rc-time span { font-size: 10px; letter-spacing: .16em; text-transform: uppercase; color: #ffd21f; }
  .rc-lap b, .rc-time b { font-weight: 400; font-size: 20px; font-variant-numeric: tabular-nums; min-width: 2ch; }
  .rc-time b { min-width: 5.6ch; }
  .rc-call { flex: 1; min-width: 0; text-align: center; color: #d7dae0; letter-spacing: .08em; text-transform: uppercase; font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .rc-lights { display: flex; gap: 8px; padding: 6px 12px; background: #000; border-radius: 999px; border: 2px solid #3a3e45; }
  .rc-lights i { width: 16px; height: 16px; border-radius: 50%; background: #2a2c31; transition: background .08s, box-shadow .08s; }
  .rc-lights .r.on { background: #ff3b30; box-shadow: 0 0 12px #ff3b30; }
  .rc-lights .y.on { background: #ffd21f; box-shadow: 0 0 12px #ffd21f; }
  .rc-lights .g.on { background: #34e07a; box-shadow: 0 0 14px #34e07a; }
  .rc-gauge { position: relative; width: 86px; height: 54px; flex: 0 0 auto; }
  .rc-gauge svg { display: block; overflow: visible; }
  .rc-gauge path { fill: none; stroke-width: 8; stroke-linecap: butt; }
  .trk { stroke: #3a3e45; }
  .red { stroke: #e5322d; stroke-dasharray: 18 100; stroke-dashoffset: -82; }
  .val { stroke: #ffd21f; filter: drop-shadow(0 0 3px rgb(255 210 31 / .6)); }
  .rc-spd { position: absolute; left: 0; right: 0; bottom: 0; text-align: center; line-height: 1; }
  .rc-spd b { display: block; font-weight: 400; font-size: 19px; font-variant-numeric: tabular-nums; }
  .rc-spd span { font-size: 8px; letter-spacing: .12em; color: #c3c7cf; text-transform: uppercase; }
  @media (prefers-reduced-motion: no-preference) {
    .rc-lap b { animation: rc-pop .45s var(--spring-snappy, ease-out); }
  }
  @keyframes rc-run { to { transform: translateX(calc(var(--w) * -1)); } }
  @keyframes rc-pop { from { transform: scale(1.5) skewX(-12deg); color: #ffd21f; } }
  @media (prefers-reduced-motion: reduce) { .rc-lines i { animation: none; } }
  @media (max-width: 900px) { .rc-call, .rc-time { display: none; } }
</style>
