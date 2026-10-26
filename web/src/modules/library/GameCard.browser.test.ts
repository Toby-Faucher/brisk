import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { Game } from '../../state/api';
import GameCard from './GameCard.svelte';

const game: Game = { appid: 400, name: 'Portal', installed: false, sizeBytes: 5e9, lastPlayed: null, playtimeMin: 0, cover: '', running: false };

describe('GameCard (not installed)', () => {
  it('one click selects it and never installs; the explicit Install button does', async () => {
    const onpick = vi.fn();
    const oninstall = vi.fn();
    const r = await render(GameCard, { props: { game, onpick, oninstall } });
    const card = page.getByRole('button', { name: /^Portal, not installed/ });
    await expect.element(card).toHaveAttribute('aria-expanded', 'false');

    await userEvent.click(card);
    expect(onpick).toHaveBeenCalledWith(game);
    expect(oninstall).not.toHaveBeenCalled();

    // The Library marks the picked card selected, which reveals Install.
    await r.rerender({ selected: true });
    await expect.element(card).toHaveAttribute('aria-expanded', 'true');
    await userEvent.click(page.getByRole('button', { name: /Install/ }));
    expect(oninstall).toHaveBeenCalledExactlyOnceWith(game);
  });
});
