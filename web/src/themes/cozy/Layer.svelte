<script lang="ts">
  import { onMount } from 'svelte';

  // The sky strip behind the header bar. Sun, moon and colours follow the real local time; it refreshes once a minute.
  // Leaves and the little neighbour are CSS only. Nothing here takes a click, and nothing draws on /stream.
  let { path }: { path: string } = $props();
  const on = $derived(!/^\/stream/.test(path));

  // [hour, top colour, bottom colour]
  const STOPS: [number, string, string][] = [
    [0, '#2d3358', '#54527f'],
    [5, '#3a3d68', '#7d6a8e'],
    [6.5, '#f5b79d', '#fde7bb'],
    [9, '#9bd5ee', '#e8f6ea'],
    [16, '#a4d9ee', '#f4f4dc'],
    [18, '#f4a98a', '#fcd9a3'],
    [19.7, '#3a3d68', '#7d6a8e'],
    [24, '#2d3358', '#54527f'],
  ];

  function sky(d: Date) {
    const t = d.getHours() + d.getMinutes() / 60;
    let i = STOPS.findIndex((s, k) => t >= s[0] && t < STOPS[k + 1][0]);
    if (i < 0) i = 0;
    const [h0, a0, b0] = STOPS[i];
    const [h1, a1, b1] = STOPS[i + 1];
    const p = Math.round(((t - h0) / (h1 - h0)) * 100);
    const day = t >= 6 && t < 18;
    // 0..1 along the arc: sunrise to sunset, or moonrise to moonset
    const arc = day ? (t - 6) / 12 : (((t + 6) % 24) / 12);
    const night = t < 5.5 || t >= 19.2;
    return {
      top: `color-mix(in oklab, ${a1} ${p}%, ${a0})`,
      bottom: `color-mix(in oklab, ${b1} ${p}%, ${b0})`,
      x: 6 + arc * 88,
      y: 70 - Math.sin(arc * Math.PI) * 62,
      day,
      night,
      label: night ? 'night' : day ? (t < 10 ? 'morning' : t < 16 ? 'day' : 'evening') : 'evening',
    };
  }

  let s = $state(sky(new Date()));
  onMount(() => {
    const tick = () => (s = sky(new Date()));
    const id = setInterval(tick, 60_000);
    document.addEventListener('visibilitychange', tick);
    return () => (clearInterval(id), document.removeEventListener('visibilitychange', tick));
  });

  const SAYS: [RegExp, string][] = [
    [/^\/library/, 'So many games. Pick a cosy one.'],
    [/^\/history/, 'Remember this one? Good times.'],
    [/^\/machines/, 'Your host is just up the road.'],
    [/^\/settings/, 'Tweak away. Nothing here bites.'],
    [/^\/$/, 'Welcome home, neighbour.'],
  ];
  const says = $derived(SAYS.find(([re]) => re.test(path))?.[1] ?? 'Nice to see you.');
</script>

{#if on}
  <div class="sky" data-night={s.night} style:--top={s.top} style:--bottom={s.bottom} aria-hidden="true">
    <div class="stars"></div>
    <div class="orb" class:moon={!s.day} style:left="{s.x}%" style:top="{s.y}px"></div>
    <i class="cloud c1"></i>
    <i class="cloud c2"></i>
    <i class="cloud c3"></i>
    <svg class="hills" viewBox="0 0 1200 40" preserveAspectRatio="none">
      <path d="M0 40V22Q80 4 190 20T400 18 620 24 840 14 1040 22 1200 12V40Z" fill="var(--hill-back)" />
      <path d="M0 40V30Q120 14 260 28T560 30 860 24 1200 30V40Z" fill="var(--hill-front)" />
    </svg>
  </div>
  <div class="fall" aria-hidden="true">
    <i class="leaf l1"></i><i class="leaf l2"></i><i class="leaf l3"></i><i class="leaf l4"></i><i class="leaf l5"></i>
  </div>
  {#key path}
    <div class="buddy" aria-hidden="true">
      <span class="say">{says}</span>
      <span class="mochi"><b class="eye l"></b><b class="eye r"></b><b class="cheek l"></b><b class="cheek r"></b></span>
    </div>
  {/key}
{/if}

<style>
  .sky { position: fixed; left: 0; right: 0; top: 0; height: var(--sky-h, 104px); z-index: 29; pointer-events: none; overflow: hidden; background: linear-gradient(var(--top), var(--bottom)); transition: background 2s; --hill-back: #a9d4a4; --hill-front: #8fc38f; }
  .sky[data-night='true'] { --hill-back: #4f6b7a; --hill-front: #3f5a64; }
  .hills { position: absolute; left: 0; bottom: -1px; width: 100%; height: 40px; }
  .orb { position: absolute; width: 38px; height: 38px; margin: -19px 0 0 -19px; border-radius: 50%; background: radial-gradient(circle at 40% 38%, #fff7c2, #ffd75e 70%); box-shadow: 0 0 0 7px #ffe27a55, 0 0 22px 10px #ffe9a066; transition: left 2s, top 2s; }
  .orb.moon { background: radial-gradient(circle at 36% 34%, #fffbe8, #e9e2c4 70%); box-shadow: 0 0 0 6px #fff8d933, 0 0 20px 8px #fff8d933; }
  .stars { position: absolute; inset: 0; opacity: 0; transition: opacity 2s; }
  .sky[data-night='true'] .stars { opacity: 1; }
  .stars::before { content: ''; position: absolute; left: 6%; top: 14px; width: 3px; height: 3px; border-radius: 50%; background: #fff; box-shadow: 120px 22px #fffc, 260px 6px #fff, 380px 30px #fffa, 560px 12px #fff, 700px 26px #fffc, 860px 8px #fff, 1020px 20px #fffa, 1180px 10px #fff, 1330px 28px #fffc, 1500px 14px #fff, 1700px 24px #fffa, 1900px 8px #fff; animation: twinkle 3.4s ease-in-out infinite; }
  .cloud { position: absolute; top: 18px; left: -160px; width: 90px; height: 24px; border-radius: 24px; background: #fffdf5; opacity: 0.92; animation: drift 80s linear infinite; }
  .cloud::before, .cloud::after { content: ''; position: absolute; background: inherit; border-radius: 50%; }
  .cloud::before { width: 38px; height: 38px; left: 14px; top: -18px; }
  .cloud::after { width: 28px; height: 28px; left: 46px; top: -11px; }
  .sky[data-night='true'] .cloud { background: #8a86b4; opacity: 0.6; }
  .c2 { top: 46px; scale: 0.7; animation-duration: 120s; animation-delay: -50s; }
  .c3 { top: 8px; scale: 0.55; animation-duration: 100s; animation-delay: -20s; }
  @keyframes drift { to { translate: calc(100vw + 340px) 0; } }
  @keyframes twinkle { 50% { opacity: 0.45; } }

  .fall { position: fixed; inset: 0; z-index: 3; pointer-events: none; overflow: hidden; }
  .leaf { position: absolute; top: -20px; width: 13px; height: 13px; border-radius: 0 80% 0 80%; background: #e8995a; opacity: 0.85; animation: fall 19s linear infinite, spin 5s ease-in-out infinite alternate; }
  .l1 { left: 12%; }
  .l2 { left: 31%; background: #9fcf8e; width: 11px; height: 11px; animation-duration: 24s, 6s; animation-delay: -9s, 0s; }
  .l3 { left: 55%; background: #f2b8a0; animation-duration: 21s, 4.5s; animation-delay: -14s, -2s; }
  .l4 { left: 77%; background: #d9a441; width: 12px; height: 12px; animation-duration: 27s, 5.5s; animation-delay: -4s, -1s; }
  .l5 { left: 92%; background: #8fc38f; width: 10px; height: 10px; animation-duration: 23s, 7s; animation-delay: -18s, -3s; }
  @keyframes fall { 0% { translate: 0 0; } 25% { translate: 36px 26vh; } 50% { translate: -28px 52vh; } 75% { translate: 30px 78vh; } 100% { translate: 0 112vh; } }
  @keyframes spin { from { rotate: -40deg; } to { rotate: 70deg; } }

  .buddy { position: fixed; right: 22px; bottom: 8px; z-index: 6; pointer-events: none; }
  .mochi { display: block; position: relative; width: 46px; height: 36px; border-radius: 46% 46% 38% 38%; background: #fff8e6; border: 2.5px solid #7a4e2d; box-shadow: inset -4px -4px 0 #f1dfb8; animation: hop 9s ease-in-out infinite; transform-origin: 50% 100%; }
  .eye { position: absolute; top: 14px; width: 5px; height: 6px; border-radius: 50%; background: #4a3426; animation: blink 6s infinite; }
  .eye.l { left: 11px; }
  .eye.r { right: 11px; }
  .cheek { position: absolute; top: 21px; width: 7px; height: 4px; border-radius: 50%; background: #f4a8a0; }
  .cheek.l { left: 4px; }
  .cheek.r { right: 4px; }
  .say { position: absolute; right: 0; bottom: 46px; width: max-content; max-width: 210px; padding: 6px 12px; border-radius: 16px; background: #fff8e6; border: 2.5px solid #7a4e2d; color: #4a3426; font: 17px/1.15 'Patrick Hand', 'Nunito Variable', ui-rounded, sans-serif; opacity: 0; scale: 0.6; transform-origin: 85% 110%; animation: chat 7s 0.6s both; }
  .say::after { content: ''; position: absolute; right: 18px; bottom: -9px; width: 12px; height: 12px; background: #fff8e6; border: 2.5px solid #7a4e2d; border-width: 0 2.5px 2.5px 0; rotate: 45deg; }
  @keyframes chat { 0% { opacity: 0; scale: 0.6; } 8% { opacity: 1; scale: 1.06; } 12% { scale: 1; } 86% { opacity: 1; scale: 1; } 100% { opacity: 0; scale: 0.9; } }
  @keyframes hop { 0%, 90%, 100% { translate: 0 0; scale: 1 1; } 93% { scale: 1.12 0.86; } 95% { translate: 0 -14px; scale: 0.94 1.1; } 98% { translate: 0 0; scale: 1.1 0.88; } }
  @keyframes blink { 0%, 94%, 100% { scale: 1 1; } 97% { scale: 1 0.1; } }
  @media (max-width: 760px) { .buddy { display: none; } }
</style>
