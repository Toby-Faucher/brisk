import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// The library is mocked: `animate` records its params and is driven by hand (`step`, `finish`).
type Anim = { p: any; obj: any; cancelled: boolean; complete(): void; cancel(): void };
const anims: Anim[] = [];
const scopes: { reverted: boolean; cleanups: (() => void)[] }[] = [];
vi.mock('animejs/animation', () => ({
  animate: (obj: any, p: any) => {
    const a: Anim = {
      p, obj, cancelled: false,
      complete() { obj.v = p.v; p.onUpdate?.(); p.onComplete?.(); },
      cancel() { a.cancelled = true; },
    };
    anims.push(a);
    return a;
  },
}));
vi.mock('animejs/svg', () => ({ createDrawable: (el: unknown) => [el] }));
const splits: any[] = [];
vi.mock('animejs/text', () => ({
  splitText: (el: any, p: any) => {
    const s = { el, p, reverted: false, words: [mkWord(), mkWord()], revert() { s.reverted = true; } };
    splits.push(s);
    return s;
  },
}));
function mkWord() {
  return { animate: vi.fn(() => ({ cancelled: false, onfinish: null as null | (() => void), cancel() { this.cancelled = true; } })) } as any;
}
vi.mock('animejs/easings', () => ({ cubicBezier: (...p: number[]) => Object.assign((t: number) => t, { p }) }));
vi.mock('animejs/scope', () => ({
  createScope: () => {
    const s = { reverted: false, cleanups: [] as (() => void)[], add(fn: (s: unknown) => void | (() => void)) { const c = fn(s); if (c) s.cleanups.push(c); return s; }, revert() { s.reverted = true; s.cleanups.forEach((c) => c()); } };
    scopes.push(s);
    return s;
  },
}));

let path = '/';
let reduce = false;
const mqListeners: ((e: { matches: boolean }) => void)[] = [];
const vars: Record<string, string> = { '--ease': 'cubic-bezier(.2, .8, .2, 1)', '--dur-count': '600ms', '--spring-snappy-dur': '700ms', '--spring-snappy': 'linear(0, 1)' };
vi.stubGlobal('location', { get pathname() { return path; } });
vi.stubGlobal('matchMedia', () => ({ get matches() { return reduce; }, addEventListener: (_: string, f: (e: { matches: boolean }) => void) => mqListeners.push(f) }));
vi.stubGlobal('document', { documentElement: {}, hidden: false, createElement: () => fakeEl() });
vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: (n: string) => vars[n] ?? '' }));
vi.stubGlobal('window', globalThis);

const m = await import('./motion');
const flush = () => new Promise((r) => setTimeout(r, 0));
function el() {
  const text = { nodeType: 3, data: '', nextSibling: null };
  const props: Record<string, string> = {};
  return { firstChild: text, text, props, style: { setProperty: (k: string, v: string) => (props[k] = v) }, animate: vi.fn(() => ({})) } as any;
}
const fmt = (v: number) => `${Math.round(v)} °C`;

beforeEach(() => ((path = '/'), (reduce = false), (anims.length = 0), (scopes.length = 0), (splits.length = 0)));
afterEach(() => vi.unstubAllEnvs());

describe('guards', () => {
  it('stream route: throws in dev, silent no-op in prod, and never loads the library', async () => {
    path = '/stream';
    expect(m.onStreamRoute()).toBe(true);
    expect(() => m.motionAllowed()).toThrow(/stream/);
    expect(() => m.load('animation')).toThrow(/stream/);
    vi.stubEnv('DEV', false);
    expect(m.motionAllowed()).toBe(false);
    let settled = false;
    void m.load('animation').then(() => (settled = true));
    const e = el();
    m.tweenNumber(e, 63, { from: 60, format: fmt });
    await flush();
    expect(settled).toBe(false);
    expect(anims).toHaveLength(0);
    expect(e.text.data).toBe('63 °C');
    expect(m.glide(e, { x: 0, y: 0, w: 10, h: 10 }, { x: 0, y: 40, w: 10, h: 10 })).toBeNull();
  });
  it('only /stream and /stream-* paths are the stream route', () => {
    expect(m.onStreamRoute('/library')).toBe(false);
    expect(m.onStreamRoute('/stream-legacy')).toBe(true); // also a stream page
  });
  it('reads tokens from CSS', () => {
    expect(m.ms('--dur-count')).toBe(600);
    expect(m.ms('--nope', 123)).toBe(123);
    vars['--x'] = '.3s';
    expect(m.ms('--x')).toBe(300);
    expect(m.easeParams()).toEqual([0.2, 0.8, 0.2, 1]);
    expect(m.easeParams('ease-out')).toEqual([0.2, 0.8, 0.2, 1]);
  });
});

describe('tweenNumber', () => {
  it('first paint and unchanged values write the end state with no animation', async () => {
    const e = el();
    m.tweenNumber(e, 61.6, { format: fmt });
    m.tweenNumber(e, 62, { from: 62, format: fmt });
    await flush();
    expect(anims).toHaveLength(0);
    expect(e.text.data).toBe('62 °C');
  });
  it('rolls from the old value, ends on exactly the formatted target, with the --ease token', async () => {
    const e = el();
    m.tweenNumber(e, 63.4, { from: 60.2, format: fmt });
    expect(e.text.data).toBe('60 °C'); // before the chunk arrives the old value stays
    await flush();
    const a = anims[0];
    expect(a.p.v).toBe(63.4);
    expect(a.p.duration).toBe(600);
    expect(a.p.ease.p).toEqual([0.2, 0.8, 0.2, 1]);
    a.obj.v = 61.7; a.p.onUpdate();
    expect(e.text.data).toBe('62 °C');
    a.complete();
    expect(e.text.data).toBe('63 °C');
  });
  it('writes a CSS custom property with prop', async () => {
    const e = el();
    m.tweenNumber(e, 50, { from: 40, format: String, prop: '--p' });
    await flush();
    anims[0].complete();
    expect(e.props['--p']).toBe('50');
  });
  it('reduced motion: instant end state', async () => {
    reduce = true;
    const e = el();
    m.tweenNumber(e, 70, { from: 40, format: fmt });
    await flush();
    expect(anims).toHaveLength(0);
    expect(e.text.data).toBe('70 °C');
  });
  it('switching to reduced motion mid-flight jumps to the end', async () => {
    const e = el();
    m.tweenNumber(e, 70, { from: 40, format: fmt });
    await flush();
    mqListeners.forEach((f) => f({ matches: true }));
    expect(e.text.data).toBe('70 °C');
  });
  it('cancel before the chunk lands never starts it; cancel mid-flight returns the shown value', async () => {
    const e = el();
    m.tweenNumber(e, 70, { from: 40, format: fmt }).cancel();
    await flush();
    expect(anims).toHaveLength(0);
    const t = m.tweenNumber(e, 70, { from: 40, format: fmt });
    await flush();
    anims[0].obj.v = 55; anims[0].p.onUpdate();
    expect(t.cancel()).toBe(55);
    expect(anims[0].cancelled).toBe(true);
  });
});

describe('count attachment (Svelte re-runs it on each change)', () => {
  it('rapid updates retarget from the value on screen; null resets; idempotent at rest', async () => {
    const e = el();
    let off = m.count(60, fmt)(e)!;
    await flush();
    expect(anims).toHaveLength(0); // first value: no animation
    off();
    off = m.count(70, fmt)(e)!;
    await flush();
    anims[0].obj.v = 64; anims[0].p.onUpdate();
    off(); // a new poll lands mid-flight
    expect(anims[0].cancelled).toBe(true);
    off = m.count(66, fmt)(e)!;
    expect(e.text.data).toBe('64 °C');
    await flush();
    expect(anims[1].obj.v).toBe(64);
    anims[1].complete();
    expect(e.text.data).toBe('66 °C');
    off();
    off = m.count(66, fmt)(e)!;
    await flush();
    expect(anims).toHaveLength(2); // same value again: nothing
    off();
    expect(m.count(null, fmt)(e)).toBeUndefined();
    m.count(80, fmt)(e);
    await flush();
    expect(anims).toHaveLength(2); // after null, a value is a first paint again
  });
});

describe('scope', () => {
  it('reverts what it made, and a revert before load means it is never created', async () => {
    const s = m.scope();
    const cleanup = vi.fn();
    await s.add(['animation'], () => cleanup);
    expect(scopes).toHaveLength(1);
    s.revert();
    expect(scopes[0].reverted).toBe(true);
    expect(cleanup).toHaveBeenCalled();
    const s2 = m.scope();
    const p = s2.add(['animation'], () => {});
    s2.revert();
    await p;
    expect(scopes).toHaveLength(1);
  });
  it('does nothing under reduced motion and throws on /stream in dev', async () => {
    reduce = true;
    const fn = vi.fn();
    await m.scope().add(['animation'], fn);
    expect(fn).not.toHaveBeenCalled();
    reduce = false;
    path = '/stream';
    await expect(m.scope().add(['animation'], fn)).rejects.toThrow(/stream/);
  });
  it('motionScope attachment returns the revert', async () => {
    const node = el();
    let p!: Promise<void>;
    const off = m.motionScope((s) => (p = s.add(['animation'], () => {})))(node);
    await p;
    off();
    expect(scopes[0].reverted).toBe(true);
  });
});

describe('glide', () => {
  it('animates translate + scale from the old box to the new one, nothing else', () => {
    const e = el();
    m.glide(e, { x: 10, y: 100, w: 200, h: 30 }, { x: 10, y: 140, w: 100, h: 30 });
    const [kf, opt] = e.animate.mock.calls[0];
    expect(kf).toEqual([{ translate: '0px -40px', scale: '2 1' }, { translate: '0 0', scale: '1 1' }]);
    expect(Object.keys(kf[0]).sort()).toEqual(['scale', 'translate']);
    expect(opt).toEqual({ duration: 700, easing: 'linear(0, 1)', pseudoElement: undefined });
  });
  it('no move, reduced motion: no animation', () => {
    const e = el();
    expect(m.glide(e, { x: 0, y: 0, w: 5, h: 5 }, { x: 0, y: 0, w: 5, h: 5 })).toBeNull();
    reduce = true;
    expect(m.glide(e, { x: 0, y: 0, w: 5, h: 5 }, { x: 0, y: 50, w: 5, h: 5 })).toBeNull();
    expect(e.animate).not.toHaveBeenCalled();
  });
});

describe('count edge values', () => {
  it('NaN/Infinity never tween; null or NaN resets a CSS prop (the ring empties)', async () => {
    const e = el();
    m.count(40, String, '--p')(e);
    m.count(Number.NaN, String, '--p')(e);
    expect(e.props['--p']).toBe('0');
    m.count(Infinity, String, '--p')(e);
    m.count(50, String, '--p')(e);
    m.count(null, String, '--p')(e);
    await flush();
    expect(anims).toHaveLength(0);
    expect(e.props['--p']).toBe('0');
  });
});

function svgEl() {
  const attrs: Record<string, string> = {};
  return { attrs, setAttribute: (k: string, v: string) => (attrs[k] = v), removeAttribute: (k: string) => delete attrs[k], style: { removeProperty: vi.fn() } } as any;
}

describe('drawIn', () => {
  it('hides the line at once, draws 0 0 -> 0 1 with --dur-draw, and leaves no dash attributes when done', async () => {
    vars['--dur-draw'] = '700ms';
    const p = svgEl();
    m.drawIn(p);
    expect(p.attrs).toEqual({ pathLength: '1000', 'stroke-dasharray': '0 1010' });
    await flush();
    expect(anims[0].obj).toEqual([p]);
    expect(anims[0].p.draw).toEqual(['0 0', '0 1']);
    expect(anims[0].p.duration).toBe(700);
    anims[0].complete();
    expect(p.attrs).toEqual({});
  });
  it('destroy mid-draw cancels and cleans; reduced motion does nothing', async () => {
    const p = svgEl();
    const off = m.drawIn(p)!;
    await flush();
    off();
    expect(anims[0].cancelled).toBe(true);
    expect(p.attrs).toEqual({});
    reduce = true;
    const q = svgEl();
    expect(m.drawIn(q)).toBeUndefined();
    expect(q.attrs).toEqual({});
  });
});

describe('fadeIn', () => {
  it('native opacity fade with a delay, cancelled on destroy; nothing under reduced motion', () => {
    const cancel = vi.fn();
    const e = { animate: vi.fn(() => ({ cancel })) } as any;
    const off = m.fadeIn(250)(e)!;
    expect(e.animate.mock.calls[0][0]).toEqual({ opacity: [0, 1] });
    expect(e.animate.mock.calls[0][1]).toMatchObject({ delay: 250, fill: 'backwards' });
    off();
    expect(cancel).toHaveBeenCalled();
    reduce = true;
    const f = { animate: vi.fn() } as any;
    expect(m.fadeIn()(f)).toBeUndefined();
    expect(f.animate).not.toHaveBeenCalled();
  });
});

describe('popIn + countUp (result chips)', () => {
  it('chips pop with opacity, translate and scale only, delayed per chip, cancelled on destroy; none under reduced motion', () => {
    const cancel = vi.fn();
    const e = { animate: vi.fn(() => ({ cancel })) } as any;
    const off = m.popIn(60)(e)!;
    const [kf, o] = e.animate.mock.calls[0];
    expect(Object.keys(kf).sort()).toEqual(['opacity', 'scale', 'translate']);
    expect(o).toMatchObject({ delay: 60, fill: 'backwards' });
    off();
    expect(cancel).toHaveBeenCalled();
    reduce = true;
    const f = { animate: vi.fn() } as any;
    expect(m.popIn()(f)).toBeUndefined();
    expect(f.animate).not.toHaveBeenCalled();
  });
  it('a result number counts up from 0 to exactly its value and keeps it when its element is torn down', async () => {
    const e = el();
    const off = m.countUp(3.46, (v) => `${v.toFixed(1)}%`)(e)!;
    expect(e.text.data).toBe('0.0%');
    await flush();
    anims[0].obj.v = 1.2; anims[0].p.onUpdate();
    expect(e.text.data).toBe('1.2%');
    anims[0].complete();
    expect(e.text.data).toBe('3.5%');
    off();
    expect(e.text.data).toBe('3.5%');
  });
  it('a result of 0, a non-number and reduced motion write the end value at once; teardown mid-count settles on it', async () => {
    const e = el();
    m.countUp(0, (v) => `${v.toFixed(1)}%`)(e);
    expect(anims).toHaveLength(0);
    expect(m.countUp(Number.NaN, String)(e)).toBeUndefined();
    reduce = true;
    const f = el();
    m.countUp(5, (v) => `${v}%`)(f);
    await flush();
    expect(anims).toHaveLength(0);
    expect(f.text.data).toBe('5%');
    reduce = false;
    const g = el();
    const off = m.countUp(5, (v) => `${Math.round(v)}%`)(g)!;
    await flush();
    anims[0].obj.v = 2; anims[0].p.onUpdate();
    off();
    expect(anims[0].cancelled).toBe(true);
    expect(g.text.data).toBe('5%');
  });
});

describe('glideSwap', () => {
  const chip = (x: number, w: number) => ({ getBoundingClientRect: () => ({ left: x, top: 0, width: w, height: 20 }), animate: vi.fn() }) as any;
  it("glides the new selection's ::before from the old one, after the change", () => {
    const a = chip(0, 40), b = chip(100, 60);
    let sel = a;
    const root = { querySelector: () => sel } as any;
    const change = vi.fn(() => (sel = b));
    m.glideSwap(root, '[aria-pressed="true"]', change);
    expect(change).toHaveBeenCalled();
    const [kf, opt] = b.animate.mock.calls[0];
    expect(kf[0]).toEqual({ translate: '-100px 0px', scale: `${40 / 60} 1` });
    expect(opt.pseudoElement).toBe('::before');
  });
  it('same item, no previous selection or no root: just the change', () => {
    const a = chip(0, 40);
    const change = vi.fn();
    m.glideSwap({ querySelector: () => a } as any, 's', change);
    m.glideSwap({ querySelector: () => null } as any, 's', change);
    m.glideSwap(undefined, 's', change);
    expect(change).toHaveBeenCalledTimes(3);
    expect(a.animate).not.toHaveBeenCalled();
  });
  it('glide refuses a zero-size previous box', () => {
    const e = el();
    expect(m.glide(e, { x: 0, y: 0, w: 0, h: 0 }, { x: 0, y: 40, w: 10, h: 10 })).toBeNull();
  });
});

describe('ringPulse', () => {
  it('one native ring, scale 1 -> 2.2 and opacity .5 -> 0 over 700 ms', () => {
    const e = el();
    expect(m.ringPulse(e)).not.toBeNull();
    const [kf, opt] = e.animate.mock.calls[0];
    expect(kf).toEqual([{ scale: 1, opacity: 0.5 }, { scale: 2.2, opacity: 0 }]);
    expect(opt).toMatchObject({ duration: 700 });
    expect(opt.iterations).toBeUndefined(); // no loop
  });
  it('nothing under reduced motion and, quietly even in dev, on /stream', () => {
    const e = el();
    reduce = true;
    expect(m.ringPulse(e)).toBeNull();
    reduce = false;
    path = '/stream';
    expect(m.ringPulse(e)).toBeNull();
    expect(e.animate).not.toHaveBeenCalled();
  });
});

function fakePath(d = '') {
  const attrs: Record<string, string> = { d };
  return { attrs, removed: false, setAttribute: (k: string, v: string) => (attrs[k] = v), getAttribute: (k: string) => attrs[k], remove() { this.removed = true; } } as any;
}
function iconSvg() {
  const p = fakePath('M7 4.5v15l13-7.5z');
  const anims: any[] = [];
  const svg = {
    p, anims, querySelector: () => p,
    animate: vi.fn((kf: any, o: any) => { const a = { kf, o, cancelled: false, onfinish: null as null | (() => void), cancel() { this.cancelled = true; } }; anims.push(a); return a; }),
  } as any;
  return svg;
}

describe('swapIcon (play <-> spinner)', () => {
  const HOME = 'M7 4.5v15l13-7.5z';
  it('first value is the end state; busy turns out, swaps d, turns in, then spins; not busy swaps back', () => {
    vars['--dur-tab'] = '180ms';
    const s = iconSvg();
    const ic = m.swapIcon(s);
    ic.to(false);
    expect(s.animate).not.toHaveBeenCalled();
    ic.to(true);
    const out = s.anims[0];
    expect(out.kf).toEqual({ opacity: [1, 0], rotate: ['0deg', '-90deg'] });
    expect(out.o.duration).toBe(90); // two halves of --dur-tab
    expect(s.p.attrs.d).toBe(HOME); // untouched until the icon is invisible
    out.onfinish();
    expect(s.p.attrs.d).toBe(m.SPIN_D);
    expect(out.cancelled).toBe(true);
    const inn = s.anims[1];
    expect(inn.kf).toEqual({ opacity: [0, 1], rotate: ['90deg', '0deg'] });
    inn.onfinish();
    expect(s.anims[2].kf).toEqual({ rotate: ['0deg', '360deg'] });
    expect(s.anims[2].o).toMatchObject({ iterations: Infinity, easing: 'linear' });
    ic.to(false); // the start failed
    expect(s.anims[2].cancelled).toBe(true);
    s.anims[3].onfinish();
    s.anims[4].onfinish();
    expect(s.p.attrs.d).toBe(HOME);
    expect(s.animate).toHaveBeenCalledTimes(5); // no spin on the way back
  });
  it('the same state again does nothing; destroy mid-swap cancels it and restores d', () => {
    const s = iconSvg();
    const ic = m.swapIcon(s);
    ic.to(false);
    ic.to(false);
    ic.to(true);
    s.anims[0].onfinish();
    ic.destroy();
    expect(s.anims.every((a: any) => a.cancelled)).toBe(true);
    expect(s.p.attrs.d).toBe(HOME);
  });
  it('first value busy: ring at once and spinning; destroy stops it', () => {
    const s = iconSvg();
    const ic = m.swapIcon(s);
    ic.to(true);
    expect(s.p.attrs.d).toBe(m.SPIN_D);
    expect(s.anims).toHaveLength(1);
    ic.destroy();
    expect(s.anims[0].cancelled).toBe(true);
    expect(s.p.attrs.d).toBe(HOME);
  });
  it('reduced motion: ring at once, no animation; switching to it mid-spin stops the turning', () => {
    reduce = true;
    const s = iconSvg();
    const ic = m.swapIcon(s);
    ic.to(false);
    ic.to(true);
    expect(s.animate).not.toHaveBeenCalled();
    expect(s.p.attrs.d).toBe(m.SPIN_D);
    reduce = false;
    const t = iconSvg();
    const ic2 = m.swapIcon(t);
    ic2.to(true);
    mqListeners.forEach((f) => f({ matches: true }));
    expect(t.anims[0].cancelled).toBe(true);
  });
});

function fakeEl() {
  const e: any = { attrs: {} as Record<string, string>, removed: false, className: '', textContent: '', setAttribute(k: string, v: string) { e.attrs[k] = v; }, remove() { e.removed = true; } };
  return e;
}
function titleEl() {
  const real: any = { textContent: 'Portal 2', style: { opacity: '', removeProperty: vi.fn(() => (real.style.opacity = '')) }, getAttribute: () => '', removeAttribute: vi.fn() };
  const h: any = { real, firstElementChild: real, appended: [] as any[], append: (x: any) => h.appended.push(x) };
  return h;
}

describe('splitIn (hero title)', () => {
  it('changes nothing until the chunk lands, then words rise out of clip masks over a copy, and the copy goes when the last lands', async () => {
    const h = titleEl();
    const off = m.splitIn(h)!;
    expect(h.appended).toHaveLength(0);
    expect(h.real.style.opacity).toBe('');
    await flush();
    const fx = h.appended[0];
    expect(fx.attrs['aria-hidden']).toBe('true'); // the real text node stays the one for screen readers
    expect(fx.textContent).toBe('Portal 2');
    expect(splits[0].p).toEqual({ words: { wrap: 'clip' }, accessible: false });
    expect(h.real.style.opacity).toBe('0'); // opacity, not visibility: it stays in the accessibility tree
    const [a, b] = splits[0].words;
    expect(a.animate.mock.calls[0][0]).toEqual({ translate: ['0 100%', '0 0'] });
    expect(a.animate.mock.calls[0][1]).toMatchObject({ duration: 500, delay: 60, fill: 'backwards' });
    expect(b.animate.mock.calls[0][1].delay).toBe(105); // 45 ms apart
    const last = b.animate.mock.results[0].value;
    last.onfinish();
    expect(fx.removed).toBe(true);
    expect(splits[0].reverted).toBe(true);
    expect(h.real.style.opacity).toBe('');
    expect(h.real.removeAttribute).toHaveBeenCalledWith('style'); // no empty style attribute left
    off();
  });
  it('destroy before the chunk lands never touches the DOM; destroy mid-way cancels and restores', async () => {
    const h = titleEl();
    m.splitIn(h)!();
    await flush();
    expect(h.appended).toHaveLength(0);
    expect(splits).toHaveLength(0);
    const g = titleEl();
    const off = m.splitIn(g)!;
    await flush();
    off();
    expect(splits[0].words.every((w: any) => w.animate.mock.results[0].value.cancelled)).toBe(true);
    expect(g.appended[0].removed).toBe(true);
    expect(splits[0].reverted).toBe(true);
    expect(g.real.style.opacity).toBe('');
  });
  it('reduced motion and an empty heading do nothing; switching to reduced motion mid-way ends it', async () => {
    reduce = true;
    expect(m.splitIn(titleEl())).toBeUndefined();
    reduce = false;
    expect(m.splitIn({ firstElementChild: null } as any)).toBeUndefined();
    const h = titleEl();
    m.splitIn(h);
    await flush();
    mqListeners.forEach((f) => f({ matches: true }));
    expect(h.appended[0].removed).toBe(true);
    expect(h.real.style.opacity).toBe('');
  });
});
