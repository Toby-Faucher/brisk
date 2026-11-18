<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // A short sticker that pops in after each page change, then leaves.
  const LINES = ['Looking good.', 'Pick something fun.', 'Nothing here is serious.', 'High score time.', 'Radical.', 'No worries.', 'Totally tubular.', 'Good choice.'];
  let note = $state('');
  let showNote = $state(false);
  let seen = 0;
  $effect(() => {
    void path;
    if (stream || seen++ === 0) return; // not on /stream, not on the first load
    note = LINES[Math.floor(Math.random() * LINES.length)];
    showNote = true;
    const t = setTimeout(() => (showNote = false), 3200);
    return () => clearTimeout(t);
  });

  // Confetti pops out of a button when it is clicked. Skipped under reduced motion and on /stream.
  onMount(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const COLORS = ['#ff5fa8', '#19c2b0', '#ffd23f', '#6c3df0', '#ff8a3d'];
    const pop = (e: MouseEvent) => {
      if (/^\/stream/.test(location.pathname)) return;
      const el = (e.target as Element | null)?.closest?.('.btn, .play');
      if (!el) return;
      const r = el.getBoundingClientRect();
      const x = e.clientX || r.left + r.width / 2;
      const y = e.clientY || r.top + r.height / 2;
      for (let i = 0; i < 14; i++) {
        const p = document.createElement('i');
        p.setAttribute('aria-hidden', 'true');
        const sz = 6 + Math.random() * 7;
        const round = i % 3 === 0;
        p.style.cssText = `position:fixed;z-index:300;pointer-events:none;left:${x}px;top:${y}px;width:${sz}px;height:${round ? sz : sz * 0.5}px;background:${COLORS[i % COLORS.length]};border-radius:${round ? '50%' : '2px'}`;
        document.body.append(p);
        const a = Math.random() * Math.PI * 2;
        const d = 40 + Math.random() * 70;
        const dx = Math.cos(a) * d;
        const dy = Math.sin(a) * d - 30;
        p.animate(
          [
            { transform: 'translate(-50%,-50%) scale(.4)', opacity: 1 },
            { transform: `translate(${dx}px,${dy}px) rotate(${Math.random() * 540}deg)`, opacity: 1, offset: 0.6 },
            { transform: `translate(${dx * 1.1}px,${dy + 60}px) rotate(${Math.random() * 720}deg)`, opacity: 0 },
          ],
          { duration: 750 + Math.random() * 250, easing: 'cubic-bezier(.2,.7,.3,1)' },
        ).onfinish = () => p.remove();
      }
    };
    addEventListener('click', pop);
    return () => removeEventListener('click', pop);
  });
</script>

{#if !stream}
  <div class="mp-sky" aria-hidden="true">
    <i class="s tri a"></i>
    <i class="s dot b"></i>
    <i class="s ring c"></i>
    <i class="s tri d"></i>
    <i class="s half e"></i>
    <i class="s dot f"></i>
    <i class="s ring g"></i>
    <i class="s tri h"></i>
    <svg class="s sq a" viewBox="0 0 120 30" width="120" height="30"><path d="M4 15 q10 -18 20 0 t20 0 t20 0 t20 0 t20 0" /></svg>
    <svg class="s sq b" viewBox="0 0 120 30" width="120" height="30"><path d="M4 15 q10 -18 20 0 t20 0 t20 0 t20 0 t20 0" /></svg>
    <svg class="s plus a" viewBox="0 0 30 30" width="30" height="30"><path d="M15 3v24M3 15h24" /></svg>
    <svg class="s plus b" viewBox="0 0 30 30" width="30" height="30"><path d="M15 3v24M3 15h24" /></svg>
    <svg class="s zig" viewBox="0 0 100 24" width="100" height="24"><path d="M3 20 L19 4 L35 20 L51 4 L67 20 L83 4 L97 20" /></svg>
  </div>
  <div class="mp-note" class:on={showNote} aria-hidden="true">{note}</div>
{/if}

<style>
  .mp-sky { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; }
  .s { position: absolute; display: block; animation: mp-float var(--t, 14s) ease-in-out infinite alternate; will-change: transform; }
  svg.s { fill: none; stroke-width: 5; stroke-linecap: round; stroke-linejoin: round; }
  .tri { width: 0; height: 0; border-left: 34px solid transparent; border-right: 34px solid transparent; border-bottom: 60px solid #19c2b0; }
  .tri.a { left: 22%; top: 8%; --t: 17s; --dx: 40px; --dy: 30px; --r: 40deg; }
  .tri.d { right: 6%; top: 40%; border-bottom-color: #ff5fa8; --t: 21s; --dx: -30px; --dy: 50px; --r: -60deg; }
  .tri.h { left: 40%; bottom: 4%; border-bottom-color: #ffd23f; --t: 15s; --dx: 50px; --dy: -30px; --r: 70deg; }
  .dot { width: 46px; height: 46px; border-radius: 50%; background: #ffd23f; }
  .dot.b { right: 12%; top: 6%; --t: 12s; --dx: -40px; --dy: 40px; --r: 0deg; }
  .dot.f { left: 4%; bottom: 14%; background: #ff5fa8; width: 64px; height: 64px; --t: 19s; --dx: 30px; --dy: -50px; --r: 0deg; }
  .ring { width: 70px; height: 70px; border-radius: 50%; border: 12px solid #6c3df0; }
  .ring.c { left: 56%; top: 3%; --t: 18s; --dx: -50px; --dy: 30px; --r: 90deg; }
  .ring.g { right: 3%; bottom: 8%; border-color: #19c2b0; --t: 23s; --dx: -30px; --dy: -40px; --r: 0deg; }
  .half { width: 80px; height: 40px; border-radius: 80px 80px 0 0; background: #ff8a3d; left: 12%; top: 52%; --t: 20s; --dx: 40px; --dy: 20px; --r: -50deg; }
  .sq.a { right: 22%; bottom: 10%; stroke: #ff5fa8; --t: 16s; --dx: -40px; --dy: -20px; --r: 12deg; }
  .sq.b { left: 8%; top: 30%; stroke: #19c2b0; --t: 22s; --dx: 30px; --dy: 40px; --r: -20deg; }
  .plus.a { left: 30%; bottom: 22%; stroke: #6c3df0; --t: 13s; --dx: 20px; --dy: -40px; --r: 180deg; }
  .plus.b { right: 30%; top: 22%; stroke: #ff8a3d; --t: 17s; --dx: -20px; --dy: 30px; --r: -180deg; }
  .zig { right: 14%; top: 64%; stroke: #6c3df0; --t: 25s; --dx: -50px; --dy: 0px; --r: 8deg; }
  .mp-note { position: fixed; z-index: 60; right: 22px; bottom: 18px; pointer-events: none; opacity: 0; translate: 0 30px; rotate: 4deg; padding: 7px 16px; border-radius: 999px; font: 700 14px/1.3 'Rubik', system-ui, sans-serif; color: #1f1147; background: #ffd23f; border: 2.5px solid #1f1147; box-shadow: 4px 4px 0 #ff5fa8; transition: opacity 0.25s, translate 0.5s cubic-bezier(0.34, 1.8, 0.5, 1), rotate 0.5s cubic-bezier(0.34, 1.8, 0.5, 1); }
  .mp-note.on { opacity: 1; translate: 0 0; rotate: -3deg; }

  @keyframes mp-float { to { transform: translate(var(--dx, 20px), var(--dy, 20px)) rotate(var(--r, 30deg)); } }
  @media (prefers-reduced-motion: reduce) { .s, .mp-note { animation: none !important; transition: none !important; } }
</style>
