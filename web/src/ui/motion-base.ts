// The part of the motion layer that loads no library: guards and CSS tokens. Small on purpose, because the Shell (which
// is also mounted on /stream) imports it; everything anime.js-backed lives in ./motion.ts. Rules: docs/ui-notes.md, Motion.

export const REDUCE = '(prefers-reduced-motion: reduce)';

/** Live: read it at the moment you animate (motion.ts also listens for changes to finish what is running). */
export const reducedMotion = () => typeof matchMedia === 'function' && matchMedia(REDUCE).matches;

export const onStreamRoute = (path = typeof location === 'undefined' ? '' : location.pathname) => /^\/stream/.test(path);

/** True when motion may run here. On /stream it throws in dev (a caller is wrong) and is a silent no-op in prod. */
export function motionAllowed(): boolean {
  if (onStreamRoute()) {
    if (import.meta.env.DEV) throw new Error('motion: nothing may animate on /stream (docs/ui-design.md section 2)');
    return false;
  }
  return !reducedMotion();
}

/** A token from tokens.css, the one source of truth: `cssVar('--spring-snappy')`, `ms('--dur-enter')`. */
export const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
export function ms(name: string, fallback = 200): number {
  const v = cssVar(name);
  const n = parseFloat(v); // the minifier writes 960ms as .96s
  return Number.isNaN(n) ? fallback : /ms$/.test(v) ? n : n * 1000;
}

export type Box = { x: number; y: number; w: number; h: number };
export const box = (el: Element): Box => {
  const r = el.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height };
};

/**
 * One soft ring sent out by a status dot when the host state changes (`<span class="halo">` sibling, resting at
 * opacity 0): native `Element.animate` on `scale` + `opacity`, no library and no loop. The Shell calls this on every
 * page, so /stream is a silent no-op as well (nothing animates there). Null when nothing ran.
 */
export function ringPulse(ring: Element): Animation | null {
  if (onStreamRoute() || reducedMotion()) return null;
  return ring.animate(
    [
      { scale: 1.3, opacity: 0.6 },
      { scale: 2.4, opacity: 0 },
    ],
    { duration: 700, easing: cssVar('--ease') },
  );
}

/**
 * The previous status colour fading out over the new one (a status dot's aria-hidden `.was` copy, resting at opacity 0):
 * a cross-fade with nothing but `opacity`, native, no loop. Silent no-op on /stream and under reduced motion.
 */
export function dotFade(was: Element): Animation | null {
  if (onStreamRoute() || reducedMotion()) return null;
  return was.animate({ opacity: [1, 0] }, { duration: 360, easing: cssVar('--ease') });
}

/** FLIP: the translate + scale (transform-origin top left) that lay the `to` box over the `from` box; null for no move or a zero box. */
export function flipDelta(from: Box, to: Box): { dx: number; dy: number; sx: number; sy: number } | null {
  if (!to.w || !to.h || !from.w || !from.h) return null; // a zero box would scale from a corner
  const dx = from.x - to.x,
    dy = from.y - to.y,
    sx = from.w / to.w,
    sy = from.h / to.h;
  if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5 && Math.abs(sx - 1) < 0.01 && Math.abs(sy - 1) < 0.01) return null;
  return { dx, dy, sx, sy };
}

/**
 * Glide a highlight that already sits at `to` from where it was (`from`, viewport boxes): native Web Animations on
 * `translate` + `scale` only, so it runs on the compositor and leaves no inline style (anime.js's waapi.animate wraps
 * the same API, and the Shell must not load it). `pseudo: '::before'` animates a highlight drawn by the selected item's
 * pseudo-element, which needs `transform-origin: 0 0`. Started before the first paint (onMount, or right after a
 * flushSync) it is never seen at the new spot first. Null when nothing ran (no move, reduced motion, /stream).
 */
export function glide(el: Element, from: Box, to: Box, o: { pseudo?: string; easing?: string } = {}): Animation | null {
  const d = motionAllowed() && flipDelta(from, to);
  if (!d) return null;
  const { dx, dy, sx, sy } = d;
  return el.animate(
    [
      { translate: `${dx}px ${dy}px`, scale: `${sx} ${sy}` },
      { translate: '0 0', scale: '1 1' },
    ],
    {
      duration: ms('--spring-snappy-dur', 400),
      easing: o.easing ?? (cssVar('--spring-snappy') || cssVar('--ease')),
      pseudoElement: o.pseudo,
    },
  );
}
