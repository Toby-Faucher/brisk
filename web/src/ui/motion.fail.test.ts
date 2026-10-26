import { describe, expect, it, vi } from 'vitest';

// A lazy chunk that fails to load (hostd restarted mid-deploy, flaky network) must leave the end state, not a stale one.
vi.mock('animejs/animation', () => {
  throw new Error('Failed to fetch dynamically imported module');
});
vi.mock('animejs/text', () => {
  throw new Error('Failed to fetch dynamically imported module');
});
vi.mock('animejs/svg', () => {
  throw new Error('Failed to fetch dynamically imported module');
});
vi.stubGlobal('location', { pathname: '/' });
vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }));
vi.stubGlobal('document', { documentElement: {} });
vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: () => '' }));

const m = await import('./motion');
const settle = () => new Promise((r) => setTimeout(r, 10));

describe('chunk load failure', () => {
  it('tweenNumber shows the new value', async () => {
    const text = { nodeType: 3, data: '', nextSibling: null };
    const e = { firstChild: text } as any;
    m.tweenNumber(e, 63, { from: 45, format: (v) => `${Math.round(v)} °C` });
    expect(text.data).toBe('45 °C');
    await settle();
    expect(text.data).toBe('63 °C');
  });
  it('countUp ends on the measured value', async () => {
    const text = { nodeType: 3, data: '', nextSibling: null };
    m.countUp(3.6, (v) => `${v.toFixed(1)}%`)({ firstChild: text } as any);
    expect(text.data).toBe('0.0%');
    await settle();
    expect(text.data).toBe('3.6%');
  });
  it('splitIn leaves the real title untouched and visible', async () => {
    const real = { textContent: 'Portal 2', style: { opacity: '', removeProperty() {} }, getAttribute: () => '', removeAttribute() {} } as any;
    const h = { firstElementChild: real, append: () => { throw new Error('no copy may be added'); } } as any;
    m.splitIn(h);
    await settle();
    expect(real.style.opacity).toBe('');
  });
  it('drawIn shows the full line', async () => {
    const attrs: Record<string, string> = {};
    const p = { setAttribute: (k: string, v: string) => (attrs[k] = v), removeAttribute: (k: string) => delete attrs[k], style: { removeProperty: () => {} } } as any;
    m.drawIn(p);
    expect(attrs['stroke-dasharray']).toBe('0 1010');
    await settle();
    expect(attrs).toEqual({});
  });
});
