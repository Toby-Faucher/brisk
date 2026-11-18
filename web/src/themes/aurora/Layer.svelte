<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // A line of "forecast" that drifts in for a few seconds after each page change, then leaves.
  const LINES = [
    'Aurora forecast: Kp 6, skies clear, lag low',
    'Solar wind steady. Frames arriving on time',
    'Geomagnetic storm watch: your library is glowing',
    'Visibility excellent. Cloud cover: only the cloud kind',
    'Northern lights tonight. Southern lights also fine',
    'Aurora alert: a very good time to play something',
    'Kp 7 and rising. Consider a snack',
    'Charged particles in good spirits',
  ];
  let note = $state('');
  let showNote = $state(false);
  let seen = 0;
  $effect(() => {
    void path;
    if (stream || seen++ === 0) return; // not on /stream, not on the first load
    note = LINES[Math.floor(Math.random() * LINES.length)];
    showNote = true;
    const t = setTimeout(() => (showNote = false), 4200);
    return () => clearTimeout(t);
  });

  // Pointer parallax and spotlight. CSS reads --px/--py (-1..1, eased) and --mx/--my (px). One rAF loop that sleeps
  // when the values have settled; nothing at all under reduced motion.
  onMount(() => {
    const root = document.documentElement;
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    let tx = 0, ty = 0, x = 0, y = 0, mx = -999, my = -999, raf = 0;
    const tick = () => {
      raf = 0;
      x += (tx - x) * 0.09;
      y += (ty - y) * 0.09;
      root.style.setProperty('--px', x.toFixed(3));
      root.style.setProperty('--py', y.toFixed(3));
      root.style.setProperty('--mx', `${mx}px`);
      root.style.setProperty('--my', `${my}px`);
      if (Math.abs(tx - x) > 0.002 || Math.abs(ty - y) > 0.002) raf = requestAnimationFrame(tick);
    };
    const move = (e: PointerEvent) => {
      if (e.pointerType === 'touch') return;
      tx = (e.clientX / innerWidth) * 2 - 1;
      ty = (e.clientY / innerHeight) * 2 - 1;
      mx = e.clientX;
      my = e.clientY;
      raf ||= requestAnimationFrame(tick);
    };

    // Cover halos: the first time a cover is hovered or focused, a copy of the cover (its art or its gradient) is
    // blurred behind it, so the glow is the cover's own colours.
    const halo = (cover: HTMLElement) => {
      const host = cover.parentElement;
      if (!host || host.querySelector(':scope > .au-halo')) return;
      const h = document.createElement('i');
      h.className = 'au-halo';
      h.setAttribute('aria-hidden', 'true');
      const img = cover.querySelector('img');
      h.style.background = img?.currentSrc ? `center / cover url("${img.currentSrc}")` : cover.style.background;
      if (host.classList.contains('g')) h.style.cssText += ';left:0;top:0;width:100%;aspect-ratio:2/3';
      else h.style.cssText += `;left:${cover.offsetLeft}px;top:${cover.offsetTop}px;width:${cover.offsetWidth}px;height:${cover.offsetHeight}px`;
      host.prepend(h);
    };
    const over = (e: Event) => {
      const c = (e.target as Element | null)?.closest?.('.g, .hero, .resume');
      const cover = c?.querySelector<HTMLElement>('.cover');
      if (cover) halo(cover);
    };
    addEventListener('pointermove', move, { passive: true });
    addEventListener('pointerover', over, { passive: true });
    addEventListener('focusin', over);
    // The Library hero and the detail cover glow from the start.
    const scan = () => document.querySelectorAll<HTMLElement>('.hero .cover').forEach(halo);
    const mo = new MutationObserver(scan);
    mo.observe(document.body, { childList: true, subtree: true });
    scan();
    return () => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerover', over);
      removeEventListener('focusin', over);
      mo.disconnect();
      cancelAnimationFrame(raf);
      for (const v of ['--px', '--py', '--mx', '--my']) root.style.removeProperty(v);
    };
  });
</script>

{#if !stream}
  <div class="au-sky" aria-hidden="true">
    <i class="au-stars"></i>
    <i class="au-blob b1"></i>
    <i class="au-blob b2"></i>
    <i class="au-blob b3"></i>
    <i class="au-blob b4"></i>
    <i class="au-curtain c1"></i>
    <i class="au-curtain c2"></i>
    <i class="au-shoot"></i>
  </div>
  <div class="au-note" class:on={showNote} aria-hidden="true"><span></span>{note}</div>
{/if}

<style>
  /* Scoped here: the sky is only ever mounted by this style. */
  .au-sky { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: radial-gradient(120% 90% at 50% 0%, #0b1424 0%, #060913 60%, #04060d 100%); }
  .au-sky i { position: absolute; display: block; }
  .au-stars { inset: 0; opacity: .7; background:
    radial-gradient(1.2px 1.2px at 8% 14%, #fff, transparent), radial-gradient(1px 1px at 17% 62%, #cfe8ff, transparent),
    radial-gradient(1.4px 1.4px at 26% 30%, #fff, transparent), radial-gradient(1px 1px at 34% 80%, #fff, transparent),
    radial-gradient(1.2px 1.2px at 43% 8%, #d9f5ff, transparent), radial-gradient(1px 1px at 52% 46%, #fff, transparent),
    radial-gradient(1.5px 1.5px at 61% 22%, #fff, transparent), radial-gradient(1px 1px at 69% 70%, #cfe8ff, transparent),
    radial-gradient(1.2px 1.2px at 77% 12%, #fff, transparent), radial-gradient(1px 1px at 84% 52%, #fff, transparent),
    radial-gradient(1.3px 1.3px at 91% 34%, #e6dcff, transparent), radial-gradient(1px 1px at 96% 86%, #fff, transparent),
    radial-gradient(1px 1px at 4% 90%, #fff, transparent), radial-gradient(1.2px 1.2px at 48% 94%, #fff, transparent),
    radial-gradient(1px 1px at 73% 40%, #fff, transparent), radial-gradient(1px 1px at 12% 40%, #fff, transparent);
    animation: au-twinkle 7s ease-in-out infinite alternate; }
  .au-blob { border-radius: 50%; will-change: transform; translate: calc(var(--px, 0) * var(--d, 20) * -1px) calc(var(--py, 0) * var(--d, 20) * -1px); }
  .b1 { --d: 36; width: 70vmax; height: 56vmax; left: -22vmax; top: -26vmax; background: radial-gradient(closest-side, rgb(46 229 157 / .46), transparent); animation: au-drift-a 38s ease-in-out infinite alternate; }
  .b2 { --d: 22; width: 66vmax; height: 52vmax; right: -24vmax; top: -18vmax; background: radial-gradient(closest-side, color-mix(in srgb, var(--accent, #5eead4) 36%, transparent), transparent); animation: au-drift-b 46s ease-in-out infinite alternate; }
  .b3 { --d: 52; width: 62vmax; height: 50vmax; right: -14vmax; bottom: -28vmax; background: radial-gradient(closest-side, rgb(139 92 246 / .5), transparent); animation: au-drift-c 52s ease-in-out infinite alternate; }
  .b4 { --d: 30; width: 50vmax; height: 40vmax; left: 8vmax; bottom: -22vmax; background: radial-gradient(closest-side, rgb(34 120 235 / .34), transparent); animation: au-drift-b 60s ease-in-out infinite alternate-reverse; }
  /* the curtains: tall soft bands that sway like the real thing */
  .au-curtain { left: -10%; width: 120%; height: 62%; top: -6%; opacity: .55; transform-origin: 50% 0; mask-image: linear-gradient(180deg, #000 0%, rgb(0 0 0 / .5) 45%, transparent 90%); }
  .c1 { background: repeating-linear-gradient(97deg, transparent 0 4%, rgb(46 229 157 / .22) 9%, rgb(46 229 157 / .5) 12%, rgb(34 211 238 / .3) 16%, rgb(34 211 238 / .08) 20%, transparent 25%); animation: au-sway 26s ease-in-out infinite alternate; }
  .c2 { opacity: .36; background: repeating-linear-gradient(83deg, transparent 0 6%, rgb(139 92 246 / .25) 11%, rgb(139 92 246 / .55) 15%, color-mix(in srgb, var(--accent, #5eead4) 45%, transparent) 19%, transparent 27%); animation: au-sway 34s ease-in-out -12s infinite alternate-reverse; }
  .au-shoot { left: 70%; top: 12%; width: 120px; height: 1.5px; opacity: 0; background: linear-gradient(90deg, transparent, #fff); rotate: 28deg; animation: au-shoot 47s 9s linear infinite; }
  .au-note { position: fixed; z-index: 60; left: 50%; bottom: 20px; translate: -50% 12px; opacity: 0; pointer-events: none; padding: 7px 16px 7px 12px; border-radius: 999px; font: 300 13px/1.3 'Outfit Variable', system-ui, sans-serif; letter-spacing: .02em; color: #e8fff7; background: rgb(8 16 28 / .78); backdrop-filter: blur(14px); border: 1px solid rgb(255 255 255 / .12); box-shadow: 0 10px 30px rgb(0 0 0 / .4); display: flex; gap: 9px; align-items: center; transition: opacity .8s ease, translate 1.1s cubic-bezier(.2, .8, .2, 1); }
  .au-note.on { opacity: 1; translate: -50% 0; }
  .au-note span { width: 7px; height: 7px; border-radius: 50%; background: #2ee59d; box-shadow: 0 0 10px #2ee59d; }

  @keyframes au-drift-a { to { transform: translate(18vmax, 12vmax) scale(1.18) rotate(14deg); } }
  @keyframes au-drift-b { to { transform: translate(-16vmax, 14vmax) scale(.9) rotate(-12deg); } }
  @keyframes au-drift-c { to { transform: translate(-20vmax, -10vmax) scale(1.12) rotate(10deg); } }
  @keyframes au-sway { from { transform: translateX(-4%) skewX(-6deg) scaleY(.92); } to { transform: translateX(5%) skewX(7deg) scaleY(1.08); } }
  @keyframes au-twinkle { from { opacity: .35; } to { opacity: .85; } }
  @keyframes au-shoot { 0%, 94% { opacity: 0; transform: translateX(0); } 95% { opacity: 1; } 100% { opacity: 0; transform: translateX(-340px) translateY(180px); } }
  @media (prefers-reduced-motion: reduce) { .au-sky *, .au-note { animation: none !important; transition: none !important; } .au-shoot { display: none; } }
</style>
