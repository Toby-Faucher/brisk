import { describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { Game } from '../state/api';
import QuitHarness from '../test/QuitHarness.svelte';

const game: Game = { appid: 620, name: 'Portal 2', installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 0, playtime2wkMin: 0, cover: '', hero: '', logo: '', achUnlocked: null, achTotal: null, running: true };

describe('Quit confirm dialog', () => {
  it('moves focus in, traps Tab, closes on Esc and gives focus back to the Quit button', async () => {
    await render(QuitHarness, { props: { game } });
    const quit = page.getByRole('button', { name: 'Quit Portal 2' });
    await userEvent.click(quit);
    const dialog = page.getByRole('dialog', { name: 'Quit Portal 2?' });
    await expect.element(dialog).toBeInTheDocument();
    await expect.poll(() => dialog.element().contains(document.activeElement)).toBe(true);

    // Tab and Shift+Tab cycle inside (Cancel <-> Quit), never back to the page
    for (const key of ['{Tab}', '{Tab}', '{Tab}', '{Shift>}{Tab}{/Shift}', '{Shift>}{Tab}{/Shift}']) {
      await userEvent.keyboard(key);
      expect(dialog.element().contains(document.activeElement), key).toBe(true);
    }

    await userEvent.keyboard('{Escape}');
    await expect.element(dialog).not.toBeInTheDocument();
    await expect.element(quit).toHaveFocus();
  });

  it('stays mounted (with its text) while its exit transition runs, then unmounts', async () => {
    // base.css is not loaded in browser tests (and motion is reduced): this injected rule is the only transition.
    const css = document.head.appendChild(document.createElement('style'));
    css.textContent = '.rs { transition: opacity 2000ms linear; } .rs[data-ending-style] { opacity: 0; }';
    try {
      await render(QuitHarness, { props: { game } });
      await userEvent.click(page.getByRole('button', { name: 'Quit Portal 2' }));
      await expect.element(page.getByRole('dialog', { name: 'Quit Portal 2?' })).toBeInTheDocument();
      await userEvent.keyboard('{Escape}');
      await new Promise((r) => setTimeout(r, 100));
      const el = document.querySelector('.rs');
      expect(el?.hasAttribute('data-ending-style')).toBe(true);
      expect(el?.textContent).toContain('Quit Portal 2?');
      await expect.poll(() => document.querySelector('.rs'), { timeout: 4000 }).toBeNull();
    } finally {
      css.remove();
    }
  });
});
