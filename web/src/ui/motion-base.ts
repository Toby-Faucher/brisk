// The part of the motion layer that loads no library: guards and CSS tokens. Small on purpose, because the Shell (which
// is also mounted on /stream) imports it; everything anime.js-backed lives in ./motion.ts. Rules: docs/ui-notes.md, Motion.

const REDUCE = '(prefers-reduced-motion: reduce)';

/** Live: read it at the moment you animate (motion.ts also listens for changes to finish what is running). */
export const reducedMotion = () => typeof matchMedia === 'function' && matchMedia(REDUCE).matches;
export const onReduceChange = (fn: (reduce: boolean) => void) => matchMedia(REDUCE).addEventListener('change', (e) => fn(e.matches));

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
  const n = parseFloat(v);
  return Number.isFinite(n) ? (/\dms$/.test(v) || !/s$/.test(v) ? n : n * 1000) : fallback;
}

/** `--ease` as numbers, for JS easings (cubic-bezier(.2, .8, .2, 1) -> [.2, .8, .2, 1]). */
export function easeParams(v = cssVar('--ease')): [number, number, number, number] {
  const m = v.match(/cubic-bezier\(([^)]+)\)/);
  const p = m ? m[1].split(',').map(Number) : [];
  return p.length === 4 && p.every(Number.isFinite) ? (p as [number, number, number, number]) : [0.2, 0.8, 0.2, 1];
}

export type Box = { x: number; y: number; w: number; h: number };

/**
 * Glide an indicator (a pill or underline that sits at `to`) from where it was (`from`): native Web Animations on
 * `translate` + `scale` only, so it runs on the compositor and leaves no inline style. Starting it before the first paint
 * (in onMount) means it is never seen at the new spot first. Returns null when nothing ran (no move, reduced motion,
 * /stream). The element must use `transform-origin: 0 0` for the scale to line up.
 */
export function glide(el: Element, from: Box, to: Box, easing = cssVar('--spring-snappy') || cssVar('--ease')): Animation | null {
  if (!motionAllowed() || !to.w || !to.h) return null;
  const dx = from.x - to.x, dy = from.y - to.y, sx = from.w / to.w, sy = from.h / to.h;
  if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5 && Math.abs(sx - 1) < 0.01 && Math.abs(sy - 1) < 0.01) return null;
  return el.animate(
    [{ translate: `${dx}px ${dy}px`, scale: `${sx} ${sy}` }, { translate: '0 0', scale: '1 1' }],
    { duration: ms('--spring-snappy-dur', 400), easing },
  );
}
