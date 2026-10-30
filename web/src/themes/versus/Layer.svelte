<script lang="ts" module>
  // The Shell remounts on every page change, so the round count lives here.
  let round = 0;
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));
  const lib = $derived(/^\/library/.test(path));

  const TITLES: Record<string, string> = {
    '/': 'Pick your move',
    '/library': 'Choose your fighter',
    '/machines': 'Meet the host',
    '/history': 'Replay the tape',
    '/settings': 'Tune your stats',
  };
  const NUM = ['ONE', 'TWO', 'THREE', 'FOUR', 'FIVE', 'SIX', 'SEVEN', 'EIGHT', 'NINE'];
  const reduce = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const n = ++round;
  const label = `Round ${NUM[(n - 1) % NUM.length]}`;
  const sub = TITLES[path.replace(/\/$/, '') || '/'] ?? 'Get ready';
  const wins = Math.min(3, n - 1);

  let fx = $state(!reduce && !stream);
  let time = $state(99);
  let hit = $state(false);
  let combo = $state(0);
  let pane: { name: string; sub: string; bg: string; src: string; off: boolean; k: number } | null = $state(null);

  onMount(() => {
    if (stream) return;
    const stops: Array<() => void> = [];
    const timer = (fn: () => void, ms: number) => {
      const t = setTimeout(fn, ms);
      stops.push(() => clearTimeout(t));
    };
    if (fx) timer(() => (fx = false), 2300);
    if (!reduce) {
      const t = setInterval(() => (time = time > 0 ? time - 1 : 99), 1000);
      stops.push(() => clearInterval(t));
      timer(() => (hit = true), fx ? 900 : 100);
    }

    // The portrait follows the card under the pointer or the keyboard focus (Library only).
    let k = 0;
    const show = (el: Element | null) => {
      const wrap = el?.closest<HTMLElement>('.wrap[data-gid]');
      const cover = wrap?.querySelector<HTMLElement>('.cover');
      if (!wrap || !cover) return;
      const img = cover.querySelector('img');
      const name = wrap.querySelector('.name')?.textContent?.trim() ?? '';
      pane = {
        name,
        sub: wrap.querySelector('.sub')?.textContent?.trim() ?? '',
        bg: cover.style.background,
        src: img?.currentSrc ?? '',
        off: !!wrap.querySelector('.g.off'),
        k: ++k,
      };
    };
    const over = (e: Event) => show(e.target as Element);
    const first = () => {
      if (!pane) show(document.querySelector('.wrap[data-gid]'));
    };
    if (lib) {
      document.addEventListener('pointerover', over);
      document.addEventListener('focusin', over);
      const t = setInterval(() => (first(), pane && clearInterval(t)), 500);
      stops.push(() => clearInterval(t));
    }

    // Toasts stack into a combo counter.
    let last = 0;
    let off = 0;
    const mo = new MutationObserver((list) => {
      const added = list.some((m) => [...m.addedNodes].some((x) => x instanceof HTMLElement && x.classList.contains('toast')));
      if (!added) return;
      combo = Date.now() - last < 4500 ? combo + 1 : 1;
      last = Date.now();
      clearTimeout(off);
      off = window.setTimeout(() => (combo = 0), 2200);
    });
    const toasts = document.querySelector('.toasts');
    if (toasts) mo.observe(toasts, { childList: true });

    return () => {
      for (const s of stops) s();
      document.removeEventListener('pointerover', over);
      document.removeEventListener('focusin', over);
      mo.disconnect();
      clearTimeout(off);
    };
  });
</script>

{#if !stream}
  <div class="vs-streaks" aria-hidden="true">{#each [0, 1, 2, 3, 4, 5, 6] as i (i)}<i style="--i:{i}"></i>{/each}</div>
  <div class="vs-hb l" aria-hidden="true"><b></b><i class:hit></i></div>
  <div class="vs-hb r" aria-hidden="true"><b></b><i></i></div>
  <div class="vs-tag l" aria-hidden="true">P1 YOU<s>{'*'.repeat(wins)}</s></div>
  <div class="vs-tag r" aria-hidden="true">P2 HOST</div>
  <div class="vs-timer" class:low={time < 10} aria-hidden="true">{String(time).padStart(2, '0')}</div>

  {#if lib && pane}
    <aside class="vs-pane" aria-hidden="true">
      {#key pane.k}
        <div class="pic pop" class:off={pane.off} style="background:{pane.bg}">
          {#if pane.src}<img src={pane.src} alt="" />{:else}<em>{pane.name.charAt(0).toUpperCase()}</em>{/if}
        </div>
      {/key}
      <div class="plate">{pane.name}</div>
      <div class="stats">
        <div>Power<span><i style="width:{40 + (pane.name.length * 13) % 55}%"></i></span></div>
        <div>Speed<span><i style="width:{35 + (pane.name.length * 29) % 60}%"></i></span></div>
        <p>{pane.sub}</p>
      </div>
    </aside>
  {/if}

  {#if combo > 0}
    {#key combo}
      <div class="vs-combo" aria-hidden="true"><b>{combo}</b><span>{combo === 1 ? 'hit' : 'hit combo'}</span></div>
    {/key}
  {/if}

  {#if fx}
    <div class="vs-fx" aria-hidden="true">
      <div class="vs-flash"></div>
      <div class="vs-half l"></div>
      <div class="vs-half r"></div>
      <svg class="vs-bolt" viewBox="0 0 400 800" preserveAspectRatio="none"><path d="M230 0 150 300 240 300 120 560 210 560 90 800 330 430 240 430 330 250 250 250 330 0Z" fill="#fff6a8" stroke="#fff" stroke-width="4" /></svg>
      <div class="vs-vs">VS</div>
      <div class="vs-banner">{label}</div>
      <div class="vs-sub">{sub}</div>
    </div>
  {/if}
{/if}
