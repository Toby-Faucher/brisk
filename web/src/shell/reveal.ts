// Load entrance: tags what is inside the first viewport so CSS (`[data-enter]`, base.css) plays a staggered, time-based
// rise instead of the scroll-driven reveal (which sits at full progress for anything already on screen).
export const MAX_STAGGER = 8; // items past the 8th share the last delay

type Rect = { top: number; bottom: number; left: number; right: number };

/** Stagger index per rect (null = outside the viewport, keeps its scroll reveal). */
export function planEnter(rects: Rect[], vw: number, vh: number): (number | null)[] {
  let n = 0;
  return rects.map((r) => (r.bottom > 0 && r.top < vh && r.right > 0 && r.left < vw ? Math.min(n++, MAX_STAGGER) : null));
}

/**
 * Tag the untagged `.reveal` / `.enter` elements in <main> that are on screen, then drop the head script's hide flag.
 * Idempotent: a tagged element never replays, so call it after any mount (route, data arrival). Not on /stream
 * (its <main> has no .page-in) and not under reduced motion. Skips what sits in a `[data-hold]` (still hidden: Home
 * before its saved card order is in), which calls this again when it shows.
 */
export function enter() {
  const main = document.querySelector('main.page-in');
  if (main && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
    const els = [...main.querySelectorAll<HTMLElement>(':is(.reveal, .enter):not([data-enter], [data-hold] *)')];
    const plan = planEnter(els.map((e) => e.getBoundingClientRect()), innerWidth, innerHeight);
    els.forEach((e, k) => {
      if (plan[k] == null) return;
      e.style.setProperty('--i', String(plan[k]));
      e.setAttribute('data-enter', '');
    });
  }
  document.documentElement.removeAttribute('data-enter-pending');
}

/** First entrance of a route: wait (max 1.5 s) until the Astro islands have hydrated, so their markup is the final DOM. */
export function enterWhenHydrated() {
  const end = performance.now() + 1500;
  const tick = () => (document.querySelector('astro-island[ssr]') && performance.now() < end ? requestAnimationFrame(tick) : enter());
  tick();
}
