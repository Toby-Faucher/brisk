import { afterEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import TagEditor from './TagEditor.svelte';

afterEach(() => page.viewport(1280, 800));

describe('TagEditor', () => {
  for (const [w, h] of [
    [1280, 800],
    [1280, 420],
    [390, 640],
  ]) {
    it(`New tag is centred and fully inside a ${w}x${h} viewport, Save and Cancel visible`, async () => {
      await page.viewport(w, h);
      await render(TagEditor, { props: { id: null, onclose: vi.fn() } });
      await expect.element(page.getByRole('dialog', { name: 'New tag' })).toBeInTheDocument();
      const r = page.getByRole('dialog').element().getBoundingClientRect();
      expect(Math.abs(r.left + r.width / 2 - innerWidth / 2)).toBeLessThan(2);
      expect(Math.abs(r.top + r.height / 2 - innerHeight / 2)).toBeLessThan(2);
      expect(r.top).toBeGreaterThanOrEqual(0);
      expect(r.bottom).toBeLessThanOrEqual(innerHeight);
      for (const name of ['Save tag', 'Cancel']) {
        const b = page.getByRole('button', { name }).element().getBoundingClientRect();
        expect(b.top).toBeGreaterThanOrEqual(r.top);
        expect(b.bottom).toBeLessThanOrEqual(r.bottom);
      }
    });
  }
});
