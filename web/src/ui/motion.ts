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
  text: () => import('animejs/text'),
};
type Mods = { [K in keyof typeof MODS]: Awaited<ReturnType<(typeof MODS)[K]>> };
type Lib = Mods['animation'] & Mods['easings'] & Mods['scope'] & Mods['svg'] & Mods['text'];
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

/**
 * Svelte attachment for a heading whose Svelte-owned text sits in its first child (`<h3 {@attach splitIn}><span>{name}</span></h3>`,
 * the heading `position: relative`): once the `splitText` chunk is loaded the words rise out of clip masks (`translate`
 * 100% -> 0, 500 ms, 45 ms apart, native Web Animations, so no inline style stays). The words are a split COPY laid over
 * the real text, so screen readers keep the one original node (the copy is aria-hidden, the real span only fades
 * out visually: opacity, not visibility) and layout cannot shift; when the last word lands the copy goes and the real text
 * shows again, which is also what a destroy, reduced motion or a chunk that fails to load gives (nothing changes until
 * the chunk is here, so a slow load never hides the title). The caller decides "once per page": the attachment itself
 * runs on every mount it is given to.
 */
export function splitIn(h: HTMLElement) {
  if (!motionAllowed()) return;
  const real = h.firstElementChild as HTMLElement | null;
  if (!real) return;
  let fx: HTMLElement | undefined;
  let s: { revert(): unknown } | undefined;
  let dead = false;
  const anims: Animation[] = [];
  const handle = { complete: () => clean() }; // reduced motion switched on mid-way
  const clean = () => {
    if (dead && !s) return;
    dead = true;
    live.delete(handle);
    anims.forEach((a) => a.cancel());
    s?.revert();
    fx?.remove();
    real.style.removeProperty('opacity');
    if (!real.getAttribute('style')) real.removeAttribute('style');
  };
  void load('text').then((lib) => {
    if (dead || reducedMotion()) return;
    fx = document.createElement('span');
    fx.setAttribute('aria-hidden', 'true');
    fx.className = 'split-fx';
    fx.textContent = real.textContent;
    h.append(fx);
    const sp = (s = lib.splitText(fx, { words: { wrap: 'clip' }, accessible: false }));
    const easing = cssVar('--ease');
    for (const [i, w] of (sp.words as HTMLElement[]).entries()) anims.push(w.animate({ translate: ['0 100%', '0 0'] }, { duration: 500, delay: 60 + i * 45, easing, fill: 'backwards' }));
    real.style.opacity = '0';
    live.add(handle);
    const last = anims.at(-1);
    if (last) last.onfinish = clean;
    else clean();
  }).catch(() => dead || clean());
  return clean;
}

/** A 270 degree ring (the spinner), one subpath like the play triangle. */
export const SPIN_D = 'M12 4A8 8 0 1 1 4 12h3A5 5 0 1 0 12 7z';

/**
 * Play triangle <-> spinner on a filled one-path icon (`svg` of `<Icon name="play">`), for a start in flight. Not an
 * `svg.morphTo`: measured, triangle -> ring crosses itself in the first frames (the point-by-length interpolation
 * pairs unrelated places on the two outlines), so it is a quick swap instead: the icon turns -90 deg and fades out, `d`
 * changes, and it turns in from +90 deg (together `--dur-tab`, native `rotate` + `opacity`, nothing loaded). While
 * `to(true)` holds, one compositor `rotate` turns the ring. The first call and reduced motion write the end state at
 * once. Svelte renders the path once and never touches its `d`. `destroy()` cancels everything and restores `d`.
 */
export function swapIcon(svg: SVGSVGElement) {
  const path = svg.querySelector('path');
  const home = path?.getAttribute('d') ?? '';
  let state: boolean | undefined;
  let cur: Animation | undefined;
  let spin: Animation | undefined;
  const handle = { complete: () => stopAll() }; // a switch to reduced motion ends it where it stands
  const stopAll = () => (cur?.cancel(), spin?.cancel(), (cur = spin = undefined), live.delete(handle));
  const settle = (on: boolean) => {
    stopAll();
    path?.setAttribute('d', on ? SPIN_D : home);
    if (on && motionAllowed()) (spin = svg.animate({ rotate: ['0deg', '360deg'] }, { duration: 900, iterations: Infinity, easing: 'linear' })), live.add(handle);
  };
  return {
    to(on: boolean) {
      if (!path || on === state) return;
      const first = state === undefined;
      state = on;
      if (first || !motionAllowed()) return settle(on);
      stopAll();
      const half = ms('--dur-tab', 180) / 2;
      const easing = cssVar('--ease');
      const out = (cur = svg.animate({ opacity: [1, 0], rotate: ['0deg', '-90deg'] }, { duration: half, easing, fill: 'forwards' }));
      live.add(handle);
      out.onfinish = () => {
        path.setAttribute('d', on ? SPIN_D : home);
        const inn = (cur = svg.animate({ opacity: [0, 1], rotate: ['90deg', '0deg'] }, { duration: half, easing }));
        out.cancel();
        inn.onfinish = () => settle(on);
      };
    },
    destroy() {
      stopAll();
      path?.setAttribute('d', home);
    },
  };
}

/** Svelte attachment: fade an element in once it mounts (native Web Animations, nothing loaded), after `delay` ms. */
export const fadeIn = (delay = 0) => (el: Element) => {
  if (!motionAllowed()) return;
  const a = el.animate({ opacity: [0, 1] }, { duration: ms('--dur-enter', 420), delay, easing: cssVar('--ease'), fill: 'backwards' });
  return () => a.cancel();
};

/** Svelte attachment: a result chip pops in when it mounts: opacity + `translate` + `scale` (native, compositor), `delay` ms in. */
export const popIn = (delay = 0) => (el: Element) => {
  if (!motionAllowed()) return;
  const a = el.animate({ opacity: [0, 1], translate: ['0 6px', '0 0'], scale: [0.9, 1] }, { duration: 240, delay, easing: cssVar('--ease'), fill: 'backwards' });
  return () => a.cancel();
};

/**
 * Svelte attachment: a result number counts up from 0 to `value` when it mounts (`tweenNumber`). Only for numbers measured
 * once and then fixed, never for a live reading. Svelte still renders the same text, so both agree at rest; with a chunk that
 * fails to load or under reduced motion the end value is simply there.
 */
export const countUp = (value: number, format: (v: number) => string) => (el: HTMLElement) => {
  if (!Number.isFinite(value)) return;
  const t = tweenNumber(el, value, { from: 0, format });
  return () => (t.cancel(), setText(el, format(value)));
};
