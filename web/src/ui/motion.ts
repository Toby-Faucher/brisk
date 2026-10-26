// The one place anime.js enters the app (docs/ui-notes.md, Motion: what uses it and the rules). Nothing here imports the
// library statically: each helper loads the sub-path it needs the first time it really animates, never on /stream and
// never under reduced motion. Page modules import this file; the Shell imports only ./motion-base.
import { flushSync } from 'svelte';
import { box, cssVar, glide, motionAllowed, ms, onStreamRoute, REDUCE, reducedMotion } from './motion-base';
import type { Scope } from 'animejs/scope';

export * from './motion-base';

const MODS = {
  animation: () => import('animejs/animation'),
  easings: () => import('animejs/easings'),
  scope: () => import('animejs/scope'),
  svg: () => import('animejs/svg'),
};
type Mods = { [K in keyof typeof MODS]: Awaited<ReturnType<(typeof MODS)[K]>> };
type Lib = Mods['animation'] & Mods['easings'] & Mods['scope'] & Mods['svg'];
/** What `load(...names)` resolves to: the union of each named module's exports (distributes over K). */
type Loaded<K extends keyof Mods> = Pick<Lib, (K extends keyof Mods ? keyof Mods[K] : never) & keyof Lib>;

/**
 * Load anime.js sub-paths on demand (the browser caches the chunks). On /stream it throws in dev and in prod returns a
 * promise that never settles, so a wrong caller simply never animates and the library is never fetched there.
 */
export function load<K extends keyof Mods>(...names: K[]): Promise<Loaded<K>> {
  if (onStreamRoute()) {
    if (import.meta.env.DEV) throw new Error('motion: anime.js must not load on /stream');
    return new Promise(() => {});
  }
  return Promise.all(names.map((n) => MODS[n]())).then((m) => Object.assign({}, ...m));
}

/** `--ease` as numbers, for JS easings (cubic-bezier(.2, .8, .2, 1) -> [.2, .8, .2, 1]). */
export function easeParams(v = cssVar('--ease')): [number, number, number, number] {
  const m = v.match(/cubic-bezier\(([^)]+)\)/);
  const p = m ? m[1].split(',').map(Number) : [];
  return p.length === 4 && p.every(Number.isFinite) ? (p as [number, number, number, number]) : [0.2, 0.8, 0.2, 1];
}

/** `--ease` as an anime.js easing function. */
export const ease = (lib: Pick<Lib, 'cubicBezier'>) => lib.cubicBezier(...easeParams());

// Running JS tweens: switching to reduced motion mid-flight jumps them to their end state.
const live = new Set<{ complete(): unknown }>();
if (typeof window !== 'undefined') matchMedia(REDUCE).addEventListener('change', (e) => e.matches && [...live].forEach((a) => a.complete()));

export type Tween = { cancel(): number };

/**
 * Roll a number shown in `el` from `from` to `to` (text, or a CSS custom property with `prop`). Writes `format(from)`
 * at once, so a slow chunk load never shows the end value first. No `from` (first paint), no change, reduced motion
 * or /stream: writes `format(to)` and is done. `cancel()` stops it where it is and returns the value it showed, so a
 * new tween can start from there (rapid updates retarget cleanly).
 */
export function tweenNumber(el: HTMLElement, to: number, o: { from?: number | null; format?: (v: number) => string; duration?: number; prop?: string } = {}): Tween {
  const format = o.format ?? String;
  const write = (v: number) => (o.prop ? el.style.setProperty(o.prop, format(v)) : setText(el, format(v)));
  let cur = o.from ?? to;
  if (o.from == null || o.from === to || !motionAllowed()) {
    write((cur = to));
    return { cancel: () => cur };
  }
  write(cur);
  let anim: { cancel(): unknown; complete(): unknown } | undefined;
  let dead = false;
  void load('animation', 'easings').then((lib) => {
    if (dead) return;
    const obj = { v: cur };
    const a = lib.animate(obj, {
      v: to,
      duration: o.duration ?? ms('--dur-count', 600),
      ease: ease(lib),
      onUpdate: () => write((cur = obj.v)),
      onComplete: () => (live.delete(a), write((cur = to))),
    });
    live.add((anim = a));
  }).catch(() => dead || write((cur = to))); // chunk failed to load: show the new value, never the stale one
  return {
    cancel() {
      dead = true;
      if (anim) live.delete(anim), anim.cancel();
      return cur;
    },
  };
}

/** Replace an element's text without replacing a Svelte-owned text node (Svelte keeps writing to the same node). */
function setText(el: HTMLElement, s: string) {
  const t = el.firstChild;
  if (t && t.nodeType === 3 && !t.nextSibling) (t as Text).data !== s && ((t as Text).data = s);
  else el.textContent = s;
}

const shown = new WeakMap<Element, number>();

/**
 * Every lazy path here falls back to its end state when the chunk fails to load (deploy restart, flaky network).
 *
 * Svelte attachment for a number that changes after load: `<b {@attach count(c, fmtTemp)}>{fmtTemp(c)}</b>`. Svelte
 * still renders the value (server render, screen readers, the final text); on each later change the tween rolls the same
 * text node from the value on screen to the new one with the same formatter, so both always agree at rest. The first
 * value and null (shown as a dash by Svelte) never animate. Re-runs on change; the cleanup cancels a tween in flight.
 */
export const count = (value: number | null | undefined, format: (v: number) => string, prop?: string) => (el: HTMLElement) => {
  if (value == null || !Number.isFinite(value)) {
    shown.delete(el);
    if (prop) el.style.setProperty(prop, '0'); // e.g. the ring empties when the host goes away
    return;
  }
  const t = tweenNumber(el, value, { from: shown.get(el), format, prop });
  shown.set(el, value);
  return () => shown.set(el, t.cancel());
};

export type MotionScope = {
  /** Run `fn` inside an anime.js scope once the library is loaded; whatever it creates is reverted with the scope. */
  add<K extends keyof Mods>(names: K[], fn: (lib: Loaded<K>, self: Scope) => void | (() => void)): Promise<void>;
  revert(): void;
};

/**
 * One anime.js `createScope` per mounted component (the Shell island remounts on every navigation, so nothing may
 * outlive its component): `revert()` stops and undoes everything it made. Nothing loads when motion is not allowed.
 */
export function scope(root?: HTMLElement): MotionScope {
  let s: Scope | undefined;
  let dead = false;
  return {
    async add(names, fn) {
      if (dead || !motionAllowed()) return;
      const lib = await load<(typeof names)[number] | 'scope'>('scope', ...names);
      if (dead || reducedMotion()) return;
      s ??= lib.createScope({ root });
      s.add((self) => fn(lib as never, self!));
    },
    revert() {
      dead = true;
      s?.revert();
    },
  };
}

/** Svelte attachment form of `scope`: `{@attach motionScope((m) => m.add(['svg'], ...))}`, reverted on destroy. */
export const motionScope = (setup: (m: MotionScope, node: HTMLElement) => void) => (node: HTMLElement) => {
  const m = scope(node);
  setup(m, node);
  return () => m.revert();
};

/** A selection change inside `root`: the `sel` item's `::before` highlight glides from the old item to the new one. */
export function glideSwap(root: Element | undefined, sel: string, change: () => void) {
  const old = root?.querySelector(sel);
  const from = old && box(old);
  change();
  if (!root || !from) return;
  flushSync();
  const now = root.querySelector(sel);
  if (now && now !== old) glide(now, from, box(now), { pseudo: '::before' });
}

const DRAW = ['pathLength', 'stroke-dasharray', 'stroke-dashoffset'];

/**
 * Svelte attachment for an SVG line: it draws itself in once, when it mounts (anime.js `createDrawable`). It is hidden
 * at once in anime's own '0 0' state, so a slow chunk load never flashes the full line first; on completion, on
 * destroy or when motion is off the attributes are removed again, so the line ends exactly as drawn without us.
 */
export function drawIn(el: SVGGeometryElement) {
  if (!motionAllowed()) return;
  const clean = () => (DRAW.forEach((a) => el.removeAttribute(a)), el.style.removeProperty('stroke-linecap'));
  el.setAttribute('pathLength', '1000');
  el.setAttribute('stroke-dasharray', '0 1010');
  let a: { cancel(): unknown; complete(): unknown } | undefined;
  let dead = false;
  void load('svg', 'animation', 'easings').then((lib) => {
    if (dead) return;
    const anim = lib.animate(lib.createDrawable(el), {
      draw: ['0 0', '0 1'],
      duration: ms('--dur-draw', 700),
      ease: ease(lib),
      onComplete: () => (live.delete(anim), clean()),
    });
    live.add((a = anim));
  }).catch(() => dead || clean()); // chunk failed to load: show the full line rather than none
  return () => {
    dead = true;
    if (a) live.delete(a), a.cancel();
    clean();
  };
}

/** Svelte attachment: fade an element in once it mounts (native Web Animations, nothing loaded), after `delay` ms. */
export const fadeIn = (delay = 0) => (el: Element) => {
  if (!motionAllowed()) return;
  const a = el.animate({ opacity: [0, 1] }, { duration: ms('--dur-enter', 420), delay, easing: cssVar('--ease'), fill: 'backwards' });
  return () => a.cancel();
};
