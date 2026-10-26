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
const docListeners: [string, () => void][] = [];
const vars: Record<string, string> = { '--ease': 'cubic-bezier(.2, .8, .2, 1)', '--dur-count': '600ms', '--spring-snappy-dur': '700ms', '--spring-snappy': 'linear(0, 1)' };
vi.stubGlobal('location', { get pathname() { return path; } });
vi.stubGlobal('matchMedia', () => ({ get matches() { return reduce; }, addEventListener: (_: string, f: (e: { matches: boolean }) => void) => mqListeners.push(f), removeEventListener: (_: string, f: (e: { matches: boolean }) => void) => mqListeners.splice(mqListeners.indexOf(f), 1) }));
vi.stubGlobal('document', { documentElement: {}, hidden: false, addEventListener: (t: string, f: () => void) => docListeners.push([t, f]), removeEventListener: (t: string, f: () => void) => docListeners.splice(docListeners.findIndex((x) => x[1] === f), 1), createElement: () => fakeEl(), createTextNode: (text: string) => ({ text }) });
vi.stubGlobal('requestAnimationFrame', (f: () => void) => setTimeout(f, 0));
vi.stubGlobal('CSSAnimation', class {});
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

beforeEach(() => ((path = '/'), (reduce = false), (anims.length = 0), (scopes.length = 0)));
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

describe('travel (connection path dot)', () => {
  const dot = () => {
    const a = { cancelled: false, paused: false, pause() { a.paused = true; }, play() { a.paused = false; }, cancel() { a.cancelled = true; } };
    return { a, animate: vi.fn(() => a) } as any;
  };
  afterEach(() => { (document as any).hidden = false; docListeners.length = 0; });
  it('one infinite linear translate loop, nothing else', () => {
    const e = dot();
    const stop = m.travel(e, 1600)!;
    const [kf, o] = e.animate.mock.calls[0];
    expect(Object.keys(kf).sort()).toEqual(['opacity', 'translate']);
    expect(kf.translate).toEqual(['-100% 0', '0 0']);
    expect(o).toEqual({ duration: 1600, iterations: Infinity, easing: 'linear' });
    expect(e.animate).toHaveBeenCalledTimes(1);
    stop();
    expect(e.a.cancelled).toBe(true);
    expect(docListeners).toHaveLength(0); // listeners removed again
  });
  it('is paused while the tab is hidden and plays again when it is back; starts paused if hidden', () => {
    const e = dot();
    const stop = m.travel(e, 1600)!;
    (document as any).hidden = true;
    docListeners.forEach(([, f]) => f());
    expect(e.a.paused).toBe(true);
    (document as any).hidden = false;
    docListeners.forEach(([, f]) => f());
    expect(e.a.paused).toBe(false);
    stop();
    (document as any).hidden = true;
    const h = dot();
    m.travel(h, 1600);
    expect(h.a.paused).toBe(true);
  });
  it('nothing under reduced motion or on /stream; switching to reduced motion stops it', () => {
    reduce = true;
    const e = dot();
    expect(m.travel(e, 1600)).toBeNull();
    reduce = false;
    path = '/stream';
    expect(m.travel(e, 1600)).toBeNull();
    expect(e.animate).not.toHaveBeenCalled();
    path = '/machines';
    m.travel(e, 1600);
    [...mqListeners].forEach((f) => f({ matches: true }));
    expect(e.a.cancelled).toBe(true);
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
  it('one native ring, scale 1.3 -> 2.4 and opacity .6 -> 0 over 700 ms', () => {
    const e = el();
    expect(m.ringPulse(e)).not.toBeNull();
    const [kf, opt] = e.animate.mock.calls[0];
    expect(kf).toEqual([{ scale: 1.3, opacity: 0.6 }, { scale: 2.4, opacity: 0 }]);
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

describe('dotFade', () => {
  it('the old dot colour fades out over the new one: opacity 1 -> 0 only, 360 ms, --ease, no loop', () => {
    const e = el();
    expect(m.dotFade(e)).not.toBeNull();
    const [kf, opt] = e.animate.mock.calls[0];
    expect(kf).toEqual({ opacity: [1, 0] });
    expect(opt).toEqual({ duration: 360, easing: 'cubic-bezier(.2, .8, .2, 1)' });
  });
  it('nothing under reduced motion and, quietly even in dev, on /stream', () => {
    const e = el();
    reduce = true;
    expect(m.dotFade(e)).toBeNull();
    reduce = false;
    path = '/stream';
    expect(m.dotFade(e)).toBeNull();
    expect(e.animate).not.toHaveBeenCalled();
  });
});

describe('fadeOnChange', () => {
  it('first value and an unchanged value do nothing; a new value fades the text in from .35 (opacity only)', () => {
    const e = el();
    m.fadeOnChange('10 min')(e);
    m.fadeOnChange('10 min')(e);
    expect(e.animate).not.toHaveBeenCalled();
    const off = m.fadeOnChange('11 min')(e);
    const [kf, opt] = e.animate.mock.calls[0];
    expect(kf).toEqual({ opacity: [0.35, 1] });
    expect(opt).toMatchObject({ duration: 360, easing: 'cubic-bezier(.2, .8, .2, 1)' });
    expect(typeof off).toBe('function');
  });
  it('reduced motion: the value is remembered but nothing animates', () => {
    const e = el();
    m.fadeOnChange('a')(e);
    reduce = true;
    m.fadeOnChange('b')(e);
    reduce = false;
    m.fadeOnChange('b')(e);
    expect(e.animate).not.toHaveBeenCalled();
  });
});

describe('flipDelta', () => {
  it('translate + scale that put the `to` box (origin top left) on the `from` box', () => {
    expect(m.flipDelta({ x: 10, y: 100, w: 200, h: 30 }, { x: 10, y: 140, w: 100, h: 30 })).toEqual({ dx: 0, dy: -40, sx: 2, sy: 1 });
  });
  it('null for no move or a zero box', () => {
    expect(m.flipDelta({ x: 0, y: 0, w: 5, h: 5 }, { x: 0, y: 0, w: 5, h: 5 })).toBeNull();
    expect(m.flipDelta({ x: 0, y: 0, w: 0, h: 5 }, { x: 9, y: 0, w: 5, h: 5 })).toBeNull();
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
  it('on /stream it is quiet (no throw in dev): the end state at once, no animation', () => {
    path = '/stream';
    const s = iconSvg();
    const ic = m.swapIcon(s);
    ic.to(false);
    expect(() => ic.to(true)).not.toThrow();
    expect(s.p.attrs.d).toBe(m.SPIN_D);
    expect(s.animate).not.toHaveBeenCalled();
  });
});

function fakeEl() {
  const e: any = {
    children: [] as any[], attrs: {} as Record<string, string>, removed: false, className: '', textContent: '',
    setAttribute(k: string, v: string) { e.attrs[k] = v; },
    append(...x: any[]) { e.children.push(...x); },
    remove() { e.removed = true; },
    animate: vi.fn(() => ({ cancelled: false, onfinish: null as null | (() => void), cancel() { this.cancelled = true; } })),
  };
  return e;
}
const frames = async (n = 4) => { for (let i = 0; i < n; i++) await flush(); };
/** A hero heading: `entrance` = the hero's CSS animations (their `finished` promises). */
function titleEl(text = 'Portal 2', entrance: Promise<unknown>[] = []) {
  const real: any = { textContent: text, style: { opacity: '', removeProperty: vi.fn(() => (real.style.opacity = '')) }, getAttribute: () => '', removeAttribute: vi.fn() };
  const hero: any = { hasAttribute: () => true, getAnimations: () => entrance.map((finished) => Object.assign(new (globalThis as any).CSSAnimation(), { finished })) };
  const h: any = { real, firstElementChild: real, appended: [] as any[], append: (x: any) => h.appended.push(x), closest: () => hero };
  return h;
}
const wordsOf = (fx: any) => fx.children.filter((c: any) => c.className === 'w').map((w: any) => w.children[0]);
const plain = (fx: any) => fx.children.map((c: any) => (c.className === 'w' ? c.children[0].textContent : c.text)).join('');

describe('splitWords', () => {
  it('words in clip wrappers, spaces as plain text, punctuation glued to its word; text only', () => {
    const fx = fakeEl();
    const w = m.splitWords('Half-Life: Alyx, 2', fx);
    expect(w.map((x) => x.textContent)).toEqual(['Half-', 'Life:', 'Alyx,', '2']);
    expect(plain(fx)).toBe('Half-Life: Alyx, 2'); // nothing lost, nothing added
    expect(fx.children.filter((c: any) => c.className === 'w').every((c: any) => c.children[0].className === 'wi')).toBe(true);
  });
  it('CJK, emoji and a name with markup characters survive as text (no HTML interpretation)', () => {
    for (const t of ['ポータル２ 最終章', 'Doom 🔥🔥 Eternal', '<img src=x onerror=alert(1)> & "quotes"', '']) {
      const fx = fakeEl();
      m.splitWords(t, fx);
      expect(plain(fx)).toBe(t);
    }
    const fx = fakeEl();
    expect(m.splitWords('🔥 a', fx).length).toBeGreaterThanOrEqual(2); // the emoji is a word of its own, so it rises too
  });
});

describe('splitIn (hero title)', () => {
  it('hides the real text at once; words rise only after the hero entrance has finished, then the copy goes', async () => {
    let end!: () => void;
    const h = titleEl('Portal 2', [new Promise<void>((r) => (end = r))]);
    const off = m.splitIn(h)!;
    expect(h.real.style.opacity).toBe('0');
    await frames();
    expect(h.appended).toHaveLength(0); // the hero is still entering
    end();
    await frames();
    const fx = h.appended[0];
    expect(fx.attrs['aria-hidden']).toBe('true'); // the real text node stays the one for screen readers
    expect(fx.className).toBe('split-fx');
    expect(plain(fx)).toBe('Portal 2');
    const [a, b] = wordsOf(fx);
    expect(a.animate.mock.calls[0][0]).toEqual({ translate: ['0 100%', '0 0'] });
    expect(a.animate.mock.calls[0][1]).toMatchObject({ duration: 500, delay: 120, fill: 'backwards' });
    expect(b.animate.mock.calls[0][1].delay).toBe(165); // 45 ms apart
    b.animate.mock.results[0].value.onfinish();
    expect(fx.removed).toBe(true);
    expect(h.real.style.opacity).toBe('');
    expect(h.real.removeAttribute).toHaveBeenCalledWith('style'); // no empty style attribute left
    off();
    expect(h.real.removeAttribute).toHaveBeenCalledTimes(1); // clean runs once
  });
  it('a hero with no entrance left starts at once; two lines of words and a name change mid-way end on the real text', async () => {
    const h = titleEl('A very long game name that wraps onto a second line');
    m.splitIn(h);
    await frames();
    const fx = h.appended[0];
    const w = wordsOf(fx);
    expect(w.length).toBe(11);
    expect(w[10].animate.mock.calls[0][1].delay).toBe(120 + 10 * 45);
    h.real.textContent = 'Another name'; // Svelte updated the real node while the copy plays
    w[10].animate.mock.results[0].value.onfinish();
    expect(fx.removed).toBe(true);
    expect(h.real.textContent).toBe('Another name');
    expect(h.real.style.opacity).toBe('');
  });
  it('destroy before it starts or mid-way leaves the plain text and cancels everything', async () => {
    const h = titleEl();
    m.splitIn(h)!();
    await frames();
    expect(h.appended).toHaveLength(0);
    expect(h.real.style.opacity).toBe('');
    const g = titleEl();
    const off = m.splitIn(g)!;
    await frames();
    off();
    expect(wordsOf(g.appended[0]).every((w: any) => w.animate.mock.results[0].value.cancelled)).toBe(true);
    expect(g.appended[0].removed).toBe(true);
    expect(g.real.style.opacity).toBe('');
  });
  it('reduced motion and an empty heading do nothing; switching to reduced motion mid-way ends it', async () => {
    reduce = true;
    expect(m.splitIn(titleEl())).toBeUndefined();
    reduce = false;
    expect(m.splitIn({ firstElementChild: null } as any)).toBeUndefined();
    const h = titleEl();
    m.splitIn(h);
    await frames();
    [...mqListeners].forEach((f) => f({ matches: true }));
    expect(h.appended[0].removed).toBe(true);
    expect(h.real.style.opacity).toBe('');
  });
});

describe('morph (shared-element view transition)', () => {
  type VT = { o: any; skipped: boolean; end(): void; skipTransition(): void; ready: Promise<void>; updateCallbackDone: Promise<void>; finished: Promise<void> };
  const doc = document as any;
  let vts: VT[] = [];
  let astro = false;
  beforeEach(() => {
    vts = [];
    astro = false;
    doc.documentElement = { hasAttribute: (a: string) => a === 'data-astro-transition' && astro };
    doc.startViewTransition = (o: any) => {
      let end!: () => void;
      const t: VT = { o, skipped: false, end: () => end(), skipTransition: () => ((t.skipped = true), end()), ready: Promise.resolve(), updateCallbackDone: Promise.resolve(), finished: new Promise<void>((r) => (end = r)) };
      vts.push(t);
      return t;
    };
  });
  afterEach(() => (m.skipMorph(), delete doc.startViewTransition));
  const cover = () => {
    const css: Record<string, string> = {};
    return {
      css,
      style: { set viewTransitionName(v: string) { css.vtn = v; }, removeProperty: (k: string) => k === 'view-transition-name' && delete css.vtn },
      getAttribute: (a: string) => (a === 'style' && css.vtn ? `view-transition-name: ${css.vtn}` : null),
      removeAttribute: vi.fn(),
    } as any;
  };

  it('names the old element, then only the new one after the update, type morph; names come off at the end', async () => {
    const a = cover(), b = cover();
    let cur = a;
    const update = vi.fn(() => (cur = b));
    const t = m.morph('sc', () => cur, update)!;
    expect(vts).toHaveLength(1);
    expect(vts[0].o.types).toEqual(['morph']);
    expect(a.css.vtn).toBe('sc');
    expect(update).not.toHaveBeenCalled(); // the browser calls it after capturing the old state
    vts[0].o.update();
    expect(update).toHaveBeenCalledOnce();
    expect([a.css.vtn, b.css.vtn]).toEqual([undefined, 'sc']);
    expect(a.removeAttribute).toHaveBeenCalledWith('style'); // no empty style="" left behind
    vts[0].end();
    await t.finished;
    await flush();
    expect(b.css.vtn).toBeUndefined();
  });

  it('plain update (no transition, no name) without support, under reduced motion, during the router transition, or with no old element', () => {
    const run = (pick: () => any) => {
      const update = vi.fn();
      expect(m.morph('sc', pick, update)).toBeNull();
      expect(update).toHaveBeenCalledOnce();
    };
    const a = cover();
    reduce = true;
    run(() => a);
    reduce = false;
    astro = true;
    run(() => a);
    astro = false;
    run(() => null);
    delete doc.startViewTransition;
    run(() => a);
    expect(vts).toHaveLength(0);
    expect(a.css.vtn).toBeUndefined();
  });

  it('a second call skips the first and takes its name off at once; the skipped update, run late, names nothing', () => {
    const a = cover(), b = cover();
    m.morph('sc', () => a, () => {});
    m.morph('sc', () => b, () => {});
    expect(vts[0].skipped).toBe(true);
    expect([a.css.vtn, b.css.vtn]).toEqual([undefined, 'sc']); // never two elements with one name
    vts[0].o.update();
    expect(a.css.vtn).toBeUndefined();
    expect(b.css.vtn).toBe('sc');
  });

  it('skipMorph (component destroyed) skips it and removes the name', () => {
    const a = cover();
    m.morph('sc', () => a, () => {});
    m.skipMorph();
    expect(vts[0].skipped).toBe(true);
    expect(a.css.vtn).toBeUndefined();
  });
});

