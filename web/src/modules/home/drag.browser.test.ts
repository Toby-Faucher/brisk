import { afterEach, describe, expect, it, vi } from 'vitest';
import { reorder } from './drag';

// Two 100x100 cards side by side in a positioned grid, each with a handle; `set` re-appends them in the given order.
function rig() {
  const bento = document.createElement('div');
  bento.style.cssText = 'position: relative; display: grid; grid-template-columns: 100px 100px; width: 200px;';
  for (const id of ['a', 'b']) bento.insertAdjacentHTML('beforeend', `<div data-card="${id}" style="height: 100px"><button data-grip aria-pressed="false">${id}</button></div>`);
  document.body.append(bento);
  // synthetic pointers are not active pointers, so capture would throw
  bento.setPointerCapture = () => {};
  bento.hasPointerCapture = () => false;
  let ids = ['a', 'b'];
  const o = {
    order: () => ids,
    set: vi.fn((next: string[]) => ((ids = next), next.forEach((id) => bento.append(bento.querySelector(`[data-card="${id}"]`)!)))),
    drop: vi.fn(),
  };
  const off = reorder(o)(bento);
  const grip = (id: string) => bento.querySelector<HTMLElement>(`[data-card="${id}"] [data-grip]`)!;
  const at = (type: string, el: Element, x: number, y: number) => {
    const r = bento.getBoundingClientRect();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, isPrimary: true, button: 0, pointerId: 7, clientX: r.left + x, clientY: r.top + y }));
  };
  return { bento, o, off, grip, at, ids: () => ids };
}
let cleanup: (() => void) | null = null;
afterEach(() => (cleanup?.(), (cleanup = null), (document.body.innerHTML = '')));

describe('Home reorder (drag.ts)', () => {
  for (const [what, fire] of [
    ['window blur', () => window.dispatchEvent(new Event('blur'))],
    ['the tab hiding', () => (vi.spyOn(document, 'hidden', 'get').mockReturnValue(true), document.dispatchEvent(new Event('visibilitychange')))],
  ] as const) {
    it(`cancels a pointer drag on ${what}: back to the start order, nothing dropped, nothing left`, () => {
      const r = rig();
      cleanup = r.off;
      r.at('pointerdown', r.grip('a'), 10, 10);
      expect(r.bento.querySelector('[data-card="a"]')!.hasAttribute('data-lifted')).toBe(true);
      r.at('pointermove', r.bento, 150, 50); // over b: a takes its place
      expect(r.ids()).toEqual(['b', 'a']);
      fire();
      vi.restoreAllMocks();
      expect(r.ids()).toEqual(['a', 'b']);
      expect(r.o.drop).not.toHaveBeenCalled();
      expect(r.bento.querySelector('[data-lifted]')).toBeNull();
      expect(r.grip('a').getAttribute('aria-pressed')).toBe('false');
      r.at('pointerup', r.bento, 150, 50); // the late release does nothing
      expect(r.o.drop).not.toHaveBeenCalled();
    });
  }

  it('leaves a keyboard pick-up alone on window blur', () => {
    const r = rig();
    cleanup = r.off;
    r.grip('a').dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    window.dispatchEvent(new Event('blur'));
    expect(r.grip('a').getAttribute('aria-pressed')).toBe('true');
  });

  it('a held Space (repeats) picks up once and does not drop; a held Enter does not drop either', () => {
    const r = rig();
    cleanup = r.off;
    const key = (key: string, repeat = false) => r.grip('a').dispatchEvent(new KeyboardEvent('keydown', { key, repeat, bubbles: true }));
    key(' ');
    key(' ', true);
    key(' ', true);
    expect(r.grip('a').getAttribute('aria-pressed')).toBe('true');
    key('Enter', true);
    expect(r.o.drop).not.toHaveBeenCalled();
    key('Enter');
    expect(r.o.drop).toHaveBeenCalledOnce();
    key(' ', true); // still held after the drop: no new pick-up
    expect(r.grip('a').getAttribute('aria-pressed')).toBe('false');
  });
});
