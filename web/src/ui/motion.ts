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

let morphing: { t: ViewTransition; untag(): void } | null = null;

/**
 * Shared-element view transition of ONE element that `update` replaces (History: a session's cover grows into the
 * detail header and back). `pick()` finds it: the old one is named `name` before, the new one after `update` (which
 * must leave the DOM done, flushSync), and only those; type `morph` keeps the root out of the capture (base.css), so the
 * rest of the page stays live and no click is swallowed. A second call skips the running one and takes its name off at
 * once (never two elements with one name); names come off when it ends, is skipped or fails. Plain `update()` without
 * support, under reduced motion, on /stream, during the router's own transition or with no old element.
 */
export function morph(name: string, pick: () => HTMLElement | null | undefined, update: () => void): ViewTransition | null {
  skipMorph();
  const old = pick();
  if (!old || !document.startViewTransition || !motionAllowed() || document.documentElement.hasAttribute('data-astro-transition')) return update(), null;
  const tagged: HTMLElement[] = [];
  const tag = (el: HTMLElement | null | undefined) => el && ((el.style.viewTransitionName = name), tagged.push(el));
  const untag = () => {
    for (const el of tagged.splice(0)) {
      el.style.removeProperty('view-transition-name');
      if (!el.getAttribute('style')) el.removeAttribute('style');
    }
  };
  tag(old);
  const me = { t: null as unknown as ViewTransition, untag };
  // A skipped transition still runs its update later: it then names nothing (the newer one owns the name).
  me.t = document.startViewTransition({ update: () => (untag(), update(), morphing === me && tag(pick())), types: ['morph'] });
  morphing = me;
  const done = () => (untag(), morphing === me && (morphing = null));
  me.t.ready.catch(() => {});
  me.t.updateCallbackDone.catch(() => {});
  me.t.finished.then(done, done);
  return me.t;
}

/** End a running `morph` now (its update still happens), e.g. when the component that started it is destroyed. */
export function skipMorph() {
  if (!morphing) return;
  morphing.t.skipTransition();
  morphing.untag();
  morphing = null;
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
 * Cut `text` into clip-masked word wrappers inside `into` (`<span class="w"><span class="wi">word</span></span>`) and
 * return the inner spans. `Intl.Segmenter` finds the words (CJK and emoji safe); whitespace stays plain text nodes,
 * punctuation sticks to the word before it. Only createElement / textContent: the text is external data, never innerHTML.
 */
export function splitWords(text: string, into: Element): HTMLElement[] {
  const words: HTMLElement[] = [];
  let glued = false; // the last child is a word with no space after it yet
  for (const { segment } of new Intl.Segmenter(undefined, { granularity: 'word' }).segment(text)) {
    if (/^\s+$/.test(segment)) {
      into.append(document.createTextNode(segment));
      glued = false;
    } else if (glued && /^\p{P}+$/u.test(segment)) {
      words[words.length - 1].textContent += segment;
    } else {
      const w = document.createElement('span');
      const wi = document.createElement('span');
      w.className = 'w';
      wi.className = 'wi';
      wi.textContent = segment;
      w.append(wi);
      into.append(w);
      words.push(wi);
      glued = true;
    }
  }
  return words;
}

/**
 * Svelte attachment for a heading whose Svelte-owned text sits in its first child (`<h3 {@attach splitIn}><span>{name}</span></h3>`,
 * the heading `position: relative`, `.split-fx .w { display: inline-block; overflow: clip }`): the words rise out of clip
 * masks (`translate` 100% -> 0, 500 ms, 45 ms apart, native Web Animations, no library, no inline style left) once the
 * hero around it has finished its own entrance (+ 120 ms), so nothing plays under a card that is still fading in. The words
 * are a split COPY laid over the real text: screen readers keep the one original node (the copy is aria-hidden; the real
 * span only fades out, opacity, so it stays in the accessibility tree) and layout cannot shift. When the last word lands
 * the copy goes and the real text shows again; so it does on destroy, on reduced motion switching on, and on any error.
 * The caller decides "once per page"; the attachment itself runs on every mount it is given to.
 */
export function splitIn(h: HTMLElement) {
  if (onStreamRoute() || reducedMotion()) return;
  const real = h.firstElementChild as HTMLElement | null;
  if (!real) return;
  let fx: HTMLElement | undefined;
  let done = false;
  const anims: Animation[] = [];
  const handle = { complete: () => clean() }; // reduced motion switched on mid-way
  const clean = () => {
    if (done) return;
    done = true;
    live.delete(handle);
    anims.forEach((a) => a.cancel());
    fx?.remove();
    real.style.removeProperty('opacity');
    if (!real.getAttribute('style')) real.removeAttribute('style');
  };
  const start = () => {
    if (done || reducedMotion()) return clean();
    try {
      fx = document.createElement('span');
      fx.setAttribute('aria-hidden', 'true');
      fx.className = 'split-fx';
      h.append(fx);
      const easing = cssVar('--ease');
      for (const [i, w] of splitWords(real.textContent ?? '', fx).entries()) anims.push(w.animate({ translate: ['0 100%', '0 0'] }, { duration: 500, delay: 120 + i * 45, easing, fill: 'backwards' }));
      const last = anims.at(-1);
      if (last) last.onfinish = clean;
      else clean();
    } catch {
      clean();
    }
  };
  // The title is hidden from the start (the hero is not visible yet anyway) and shown again by clean().
  real.style.opacity = '0';
  live.add(handle);
  const hero = h.closest('.hero');
  let tries = 30; // frames to wait for the hero's entrance to be tagged (enter() in shell/reveal.ts), then start regardless
  const wait = () => {
    if (done) return;
    if (hero && !hero.hasAttribute('data-enter') && tries-- > 0) return void requestAnimationFrame(wait);
    const css = hero ? hero.getAnimations().filter((a) => a instanceof CSSAnimation) : [];
    void Promise.allSettled(css.map((a) => a.finished)).then(start);
  };
  requestAnimationFrame(wait);
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
    if (on && !onStreamRoute() && !reducedMotion()) (spin = svg.animate({ rotate: ['0deg', '360deg'] }, { duration: 900, iterations: Infinity, easing: 'linear' })), live.add(handle);
  };
  return {
    to(on: boolean) {
      if (!path || on === state) return;
      const first = state === undefined;
      state = on;
      if (first || onStreamRoute() || reducedMotion()) return settle(on);
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

const lastText = new WeakMap<Element, string>();

/**
 * Svelte attachment for a text that changes after load (Machines uptime, Sunshine state): `<dd {@attach fadeOnChange(v)}>{v}</dd>`.
 * Svelte writes the text; on a later change the element only fades in from .35 (opacity, native, nothing loaded), so nothing
 * moves and nothing is announced. The first value, an unchanged value and reduced motion do nothing.
 */
export const fadeOnChange = (value: string) => (el: Element) => {
  const was = lastText.get(el);
  lastText.set(el, value);
  if (was === undefined || was === value || !motionAllowed()) return;
  const a = el.animate({ opacity: [0.35, 1] }, { duration: 360, easing: cssVar('--ease') });
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


/**
 * A packet dot travelling along a lane forever (`el` is as wide as the lane with the dot at its right edge, inside an
 * `overflow: hidden` lane): ONE native compositor animation of `translate` (percent of its own width, so nothing is
 * measured and a resize needs nothing), paused while the tab is hidden, ended by reduced motion switching on. Returns
 * the stop function, or null when nothing runs (reduced motion, /stream); the caller calls it on destroy.
 */
export function travel(el: Element, ms: number): (() => void) | null {
  if (onStreamRoute() || reducedMotion()) return null;
  const a = el.animate({ translate: ['-100% 0', '0 0'], opacity: [1, 1] }, { duration: ms, iterations: Infinity, easing: 'linear' });
  const mq = matchMedia(REDUCE);
  const sync = () => (document.hidden ? a.pause() : a.play());
  const stop = () => {
    a.cancel();
    document.removeEventListener('visibilitychange', sync);
    mq.removeEventListener('change', onMq);
  };
  const onMq = (e: { matches: boolean }) => e.matches && stop();
  sync();
  document.addEventListener('visibilitychange', sync);
  mq.addEventListener('change', onMq);
  return stop;
}

