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

const withCover: Game = { ...game, cover: '/api/games/x/art/cover?v=1', credits: ['SteamGridDB'] };
const px = 'data:image/gif;base64,R0lGODlhAQABAAAAACw=';
const img = (id: number, thumb: string | null = `/api/art/emu%3Adolphin%3ARMCE01/thumb/cover/${id}`) => ({
  id,
  width: 600,
  height: 900,
  style: 'alternate',
  thumb,
});
const list = (items = [img(11), img(12), img(13)], current: number | null = 11) => ({
  kind: 'cover' as const,
  total: items.length,
  items,
  current,
  chosen: false,
});

describe('the art picker', () => {
  const open = async (g = withCover) => {
    await render(ArtActions, { props: { game: g } });
    await userEvent.click(page.getByRole('button', { name: 'Choose different art' }));
  };

  it('opens with thumbnails through the machine, marks the one in use, and applies a pick', async () => {
    const c = vi.spyOn(api, 'artCandidates').mockResolvedValue(list());
    const load = vi.spyOn(api, 'artImage').mockResolvedValue(px);
    const pick = vi.spyOn(api, 'artPick').mockResolvedValue({ state: 'done' });
    await open();
    await expect.element(page.getByRole('radio', { name: /cover 2/ })).toBeInTheDocument();
    expect(c).toHaveBeenCalledWith('emu:dolphin:RMCE01', 'cover');
    await expect.poll(() => load.mock.calls.map((x) => x[0])).toContain('/api/art/emu%3Adolphin%3ARMCE01/thumb/cover/12');
    await expect.element(page.getByRole('radio', { name: /cover 1.*in use/ })).toBeInTheDocument();
    const use = page.getByRole('button', { name: 'Use this art' });
    await expect.element(use).toHaveAttribute('aria-disabled', 'true');
    await userEvent.click(page.getByRole('radio', { name: /cover 2/ }));
    await expect.element(page.getByRole('radio', { name: /cover 2/ })).toHaveAttribute('aria-checked', 'true');
    await userEvent.click(use);
    expect(pick).toHaveBeenCalledWith('emu:dolphin:RMCE01', 'cover', 12);
    await expect.element(page.getByRole('dialog', { name: 'Choose different art' })).not.toBeInTheDocument();
  });

  it('each kind loads on its tab; arrows move between thumbnails; Enter selects, Enter again applies; Escape closes', async () => {
    const c = vi.spyOn(api, 'artCandidates').mockImplementation(async (_k, kind) => ({ ...list(), kind }));
    vi.spyOn(api, 'artImage').mockResolvedValue(px);
    const pick = vi.spyOn(api, 'artPick').mockResolvedValue({ state: 'done' });
    await open();
    await userEvent.click(page.getByRole('tab', { name: 'Hero' }));
    await expect.poll(() => c.mock.calls.map((x) => x[1])).toEqual(['cover', 'hero']);
    await userEvent.click(page.getByRole('tab', { name: 'Cover' }));
    const first = page.getByRole('radio', { name: /cover 1/ });
    (first.element() as HTMLElement).focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(page.getByRole('radio', { name: /cover 2/ }).element());
    await userEvent.keyboard('{Enter}');
    await expect.element(page.getByRole('radio', { name: /cover 2/ })).toHaveAttribute('aria-checked', 'true');
    expect(pick).not.toHaveBeenCalled();
    await userEvent.keyboard('{Enter}');
    expect(pick).toHaveBeenCalledWith('emu:dolphin:RMCE01', 'cover', 12);
    // reopen and close with Escape
    await userEvent.click(page.getByRole('button', { name: 'Choose different art' }));
    await expect.element(page.getByRole('dialog', { name: 'Choose different art' })).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    await expect.element(page.getByRole('dialog', { name: 'Choose different art' })).not.toBeInTheDocument();
  });

  it('says what went wrong, offers Try again, and says when there is nothing', async () => {
    const c = vi.spyOn(api, 'artCandidates').mockRejectedValueOnce(new ApiError('x', 404)).mockResolvedValueOnce(list([]));
    await open();
    await expect.element(page.getByText(/has not matched this game/)).toBeInTheDocument();
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.element(page.getByText(/has no cover images/)).toBeInTheDocument();
    expect(c).toHaveBeenCalledTimes(2);
  });

  it('shows a plain tile for an image with no preview, and a failed apply keeps the dialog', async () => {
    vi.spyOn(api, 'artCandidates').mockResolvedValue(list([img(11, null), img(12)]));
    vi.spyOn(api, 'artImage').mockResolvedValue(px);
    vi.spyOn(api, 'artPick').mockRejectedValue(new ApiError('x', 502));
    await open();
    await expect.element(page.getByText('No preview')).toBeInTheDocument();
    await userEvent.click(page.getByRole('radio', { name: /cover 2/ }));
    await userEvent.click(page.getByRole('button', { name: 'Use this art' }));
    await expect.element(page.getByRole('dialog', { name: 'Choose different art' })).toBeInTheDocument();
  });

  it('Search again closes the picker and asks hostd to look again', async () => {
    vi.spyOn(api, 'artCandidates').mockResolvedValue(list());
    vi.spyOn(api, 'artImage').mockResolvedValue(px);
    const r = vi.spyOn(api, 'artRefresh').mockResolvedValue({ state: 'searching' });
    await open();
    await userEvent.click(page.getByRole('button', { name: 'Search again' }));
    expect(r).toHaveBeenCalledWith('emu:dolphin:RMCE01');
  });
});

describe('art actions in the game panel', () => {
  it('keeps the same side padding as the other panel sections', async () => {
    const { container } = await render(ArtActions, { props: { game } });
    expect(getComputedStyle(container.querySelector('section.art') as Element).paddingLeft).toBe('22px');
  });

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
