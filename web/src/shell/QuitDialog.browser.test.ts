import { describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { Game } from '../state/api';
import QuitHarness from '../test/QuitHarness.svelte';

const game: Game = { appid: 620, name: 'Portal 2', installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 0, cover: '', running: true };

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
});
