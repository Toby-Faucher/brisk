import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { navigate } from 'astro:transitions/client';
import type { Game } from '../state/api';
import { games } from '../state/games';
import { notes } from '../state/notes';
import CommandMenu from './CommandMenu.svelte';

let before: HTMLButtonElement;
afterEach(() => before?.remove());

async function setup() {
  before = Object.assign(document.createElement('button'), { textContent: 'was focused' });
  document.body.append(before);
  before.focus();
  await render(CommandMenu);
}

describe('CommandMenu', () => {
  it('Ctrl+K opens it with focus in the search box; typing filters', async () => {
    await setup();
    await userEvent.keyboard('{Control>}k{/Control}');
    const input = page.getByRole('combobox', { name: 'Search actions' });
    await expect.element(input).toHaveFocus();
    await expect.element(page.getByRole('option', { name: /Settings/ })).toBeInTheDocument();
    await userEvent.keyboard('libr');
    await expect.element(page.getByRole('option', { name: /Library/ })).toBeInTheDocument();
    await expect.element(page.getByRole('option', { name: /Settings/ })).not.toBeInTheDocument();
  });

  it('Enter runs the highlighted item (navigates) and closes', async () => {
    await setup();
    await userEvent.keyboard('{Control>}k{/Control}');
    await userEvent.keyboard('libr');
    await userEvent.keyboard('{Enter}');
    expect(navigate).toHaveBeenCalledWith('/library');
    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument();
  });

  it('Esc closes it and focus returns to what had it', async () => {
    await setup();
    await userEvent.keyboard('{Control>}k{/Control}');
    await expect.element(page.getByRole('dialog')).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument();
    await expect.element(before).toHaveFocus();
  });
});

describe('CommandMenu tags and games', () => {
  const game = (appid: number, name: string, o: Partial<Game> = {}): Game => ({
    appid,
    name,
    installed: true,
    sizeBytes: null,
    lastPlayed: null,
    playtimeMin: 100,
    playtime2wkMin: 0,
    cover: '',
    hero: '',
    logo: '',
    achUnlocked: null,
    achTotal: null,
    updated: false,
    update: null,
    controller: null,
    coop: false,
    running: false,
    ...o,
  });
  beforeEach(() => {
    vi.mocked(navigate).mockClear();
    games.set({ games: [game(1, 'Alpha Quest'), game(2, 'Bravo Run', { playtimeMin: 0, coop: true })], ownedAvailable: true });
    notes.set({ tags: { abc: { name: 'Couch night', color: 'red', symbol: 'star', note: '' } }, games: {} });
  });

  it('choosing a tag entry navigates to the filtered Library', async () => {
    await setup();
    await userEvent.keyboard('{Control>}k{/Control}');
    await userEvent.keyboard('couch');
    await userEvent.keyboard('{Enter}');
    expect(navigate).toHaveBeenCalledWith('/library?tag=abc');
  });

  it('smart tags show their count', async () => {
    await setup();
    await userEvent.keyboard('{Control>}k{/Control}');
    await userEvent.keyboard('smart tag');
    await expect.element(page.getByRole('option', { name: /Smart tag: Co-op \(1\)/ })).toBeInTheDocument();
  });

  it('Surprise me opens the only Backlog game', async () => {
    await setup();
    await userEvent.keyboard('{Control>}k{/Control}');
    await userEvent.keyboard('surprise');
    await userEvent.keyboard('{Enter}');
    expect(navigate).toHaveBeenCalledWith('/library?open=2');
  });

  it('choosing a game opens its panel', async () => {
    await setup();
    await userEvent.keyboard('{Control>}k{/Control}');
    await userEvent.keyboard('alpha');
    await userEvent.keyboard('{Enter}');
    expect(navigate).toHaveBeenCalledWith('/library?open=1');
  });
});
