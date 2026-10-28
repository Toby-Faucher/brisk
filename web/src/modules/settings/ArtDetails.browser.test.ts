import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, ApiError, type ArtQueue } from '../../state/api';
import ArtDetails from './ArtDetails.svelte';

const queue = (over: Partial<ArtQueue> = {}): ArtQueue => ({
  hasKey: true,
  credits: ['SteamGridDB'],
  items: [
    {
      id: 'emu:dolphin:RMCE01',
      title: 'Mario Kart Wii',
      candidates: [
        { id: 1, name: 'Mario Kart 8', year: 2014 },
        { id: 2, name: 'Mario Kart: Double Dash', year: null, verified: true },
      ],
    },
  ],
  ...over,
});
afterEach(() => vi.restoreAllMocks());

describe('Art and details', () => {
  it('lists the candidates and sends the pick', async () => {
    vi.spyOn(api, 'artPending').mockResolvedValue(queue());
    const ans = vi.spyOn(api, 'artAnswer').mockResolvedValue({ state: 'fetching' });
    await render(ArtDetails);
    await expect.element(page.getByText('Mario Kart 8')).toBeInTheDocument();
    await expect.element(page.getByText('2014')).toBeInTheDocument();
    await expect.element(page.getByText('Nothing is fetched', { exact: false })).toBeInTheDocument();
    await userEvent.click(page.getByRole('button', { name: 'Choose Mario Kart: Double Dash for Mario Kart Wii' }));
    expect(ans).toHaveBeenCalledWith('emu:dolphin:RMCE01', { candidateId: 2 });
    await expect.element(page.getByText('No games are waiting for a choice.')).toBeInTheDocument();
  });

  it('None of these is sent as none', async () => {
    vi.spyOn(api, 'artPending').mockResolvedValue(queue());
    const ans = vi.spyOn(api, 'artAnswer').mockResolvedValue({ state: 'none' });
    await render(ArtDetails);
    await userEvent.click(page.getByRole('button', { name: 'None of these' }));
    expect(ans).toHaveBeenCalledWith('emu:dolphin:RMCE01', { none: true });
  });

  it('keeps the card when the answer fails', async () => {
    vi.spyOn(api, 'artPending').mockResolvedValue(queue());
    vi.spyOn(api, 'artAnswer').mockRejectedValue(new ApiError('x', 500));
    await render(ArtDetails);
    await userEvent.click(page.getByRole('button', { name: 'None of these' }));
    await expect.element(page.getByText('Mario Kart 8')).toBeInTheDocument();
  });

  it('says when the machine has no key', async () => {
    vi.spyOn(api, 'artPending').mockResolvedValue(queue({ hasKey: false, items: [] }));
    await render(ArtDetails);
    await expect.element(page.getByText('Art needs a SteamGridDB key on the machine.')).toBeInTheDocument();
  });

  it('says so when the machine is an old hostd', async () => {
    vi.spyOn(api, 'artPending').mockRejectedValue(new ApiError('404', 404));
    await render(ArtDetails);
    await expect.element(page.getByText("This machine's Brisk is too old for art. Update it first.")).toBeInTheDocument();
  });
});
