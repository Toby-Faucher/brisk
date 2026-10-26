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
vi.stubGlobal('document', { documentElement: {} });
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
  it('/streaming-like paths elsewhere are not the stream route', () => {
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
