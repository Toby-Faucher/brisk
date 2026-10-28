import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, ApiError, type Game } from '../../state/api';
import ArtActions from './ArtActions.svelte';
import Cover from './Cover.svelte';

const caps = { install: false, controls: true, achievements: false, launchOptions: false, saves: false, screenshots: false, art: true };
const game: Game = {
  id: 'emu:dolphin:RMCE01',
  source: 'emu',
  emulator: 'dolphin',
  caps,
  appid: 3907440760,
  name: 'Mario Kart Wii',
  installed: true,
  sizeBytes: null,
  lastPlayed: null,
  playtimeMin: 0,
  playtime2wkMin: 0,
  cover: '',
  hero: '',
  logo: '',
  running: false,
  achUnlocked: null,
  achTotal: null,
  updated: false,
  update: null,
  controller: null,
  coop: false,
};
afterEach(() => vi.restoreAllMocks());

describe('art actions in the game panel', () => {
  it('Find art asks hostd to look, once', async () => {
    const r = vi.spyOn(api, 'artRefresh').mockResolvedValue({ state: 'searching' });
    await render(ArtActions, { props: { game } });
    await userEvent.click(page.getByRole('button', { name: 'Find art' }));
    expect(r).toHaveBeenCalledWith('emu:dolphin:RMCE01');
  });

  it('with art it says Choose different art and credits SteamGridDB', async () => {
    await render(ArtActions, {
      props: {
        game: { ...game, cover: '/api/games/x/art/cover?v=1', credits: ['SteamGridDB'], details: { year: 2008, publisher: 'Nintendo' } },
      },
    });
    await expect.element(page.getByRole('button', { name: 'Choose different art' })).toBeInTheDocument();
    await expect.element(page.getByRole('link', { name: 'SteamGridDB' })).toHaveAttribute('href', 'https://www.steamgriddb.com');
    await expect.element(page.getByText('2008 · Nintendo')).toBeInTheDocument();
  });

  it('shows the plain reason when there is no key', async () => {
    vi.spyOn(api, 'artRefresh').mockRejectedValue(new ApiError('x', 409));
    await render(ArtActions, { props: { game } });
    await userEvent.click(page.getByRole('button', { name: 'Find art' }));
    expect(api.artRefresh).toHaveBeenCalled();
  });

  it('says a game is waiting for a choice', async () => {
    await render(ArtActions, { props: { game: { ...game, needsMatch: true } } });
    await expect.element(page.getByRole('link', { name: 'Pick one in Settings' })).toBeInTheDocument();
  });

  it('is absent for an old hostd and for Steam games', async () => {
    const { caps: _c, ...old } = game;
    await render(ArtActions, { props: { game: old } });
    expect(document.querySelector('[aria-label="Art and details"]')).toBeNull();
  });

  it('a cover on the machine is loaded with the auth headers', async () => {
    const load = vi.spyOn(api, 'artImage').mockResolvedValue('data:image/gif;base64,R0lGODlhAQABAAAAACw=');
    await render(Cover, { props: { game: { ...game, cover: '/api/games/emu%3Adolphin%3ARMCE01/art/cover?v=1' } } });
    await expect.poll(() => document.querySelector('img')?.getAttribute('src')).toContain('data:image/gif');
    expect(load).toHaveBeenCalledWith('/api/games/emu%3Adolphin%3ARMCE01/art/cover?v=1');
  });

  it('a Steam CDN cover is used as it is', async () => {
    const load = vi.spyOn(api, 'artImage');
    await render(Cover, { props: { game: { ...game, cover: 'https://cdn.example/c.jpg' } } });
    await expect.poll(() => document.querySelector('img')?.getAttribute('src')).toBe('https://cdn.example/c.jpg');
    expect(load).not.toHaveBeenCalled();
  });
});
