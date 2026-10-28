import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, ApiError, type ScanResult } from '../../state/api';
import ScanGames from './ScanGames.svelte';

const res: ScanResult = {
  scanned: 9,
  added: [],
  aliased: 1,
  duplicates: 2,
  unplayable: 0,
  removed: 1,
  unknown: 1,
  broken: 0,
  truncated: false,
};
afterEach(() => vi.restoreAllMocks());

describe('Scan games folder', () => {
  it('shows the summary in plain words after a scan', async () => {
    const scan = vi.spyOn(api, 'scanLibrary').mockResolvedValue({ ...res, added: [{}, {}] as never });
    vi.spyOn(api, 'games').mockResolvedValue({ games: [], ownedAvailable: true });
    await render(ScanGames, { props: { machine: 'a' } });
    await userEvent.click(page.getByRole('button', { name: 'Scan games folder' }));
    await expect.element(page.getByRole('group', { name: 'Scan result' }).or(page.getByLabelText('Scan result'))).toBeInTheDocument();
    const t = document.querySelector('.sum')?.textContent ?? '';
    expect(t).toContain('Added2');
    expect(t).toContain('Already there3');
    expect(t).toContain('Skipped, you removed them1');
    expect(t).toContain('Not recognised1');
    expect(t).not.toContain('Damaged');
    expect(scan).toHaveBeenCalledWith('a', false);
  });

  it('can ask for removed games too', async () => {
    const scan = vi.spyOn(api, 'scanLibrary').mockResolvedValue(res);
    await render(ScanGames);
    await userEvent.click(page.getByRole('checkbox', { name: 'Also bring back games I removed' }));
    await userEvent.click(page.getByRole('button', { name: 'Scan games folder' }));
    await expect.poll(() => scan.mock.calls).toEqual([[null, true]]);
  });

  it('says what happened on a refusal, in an alert-free live region', async () => {
    vi.spyOn(api, 'scanLibrary').mockRejectedValue(new ApiError('scanned a moment ago', 429));
    await render(ScanGames);
    await userEvent.click(page.getByRole('button', { name: 'Scan games folder' }));
    await expect.element(page.getByRole('status')).toHaveTextContent('It was scanned a moment ago. Try again in a few seconds.');
  });

  it('says when the folder has no game files', async () => {
    vi.spyOn(api, 'scanLibrary').mockResolvedValue({ ...res, scanned: 0, aliased: 0, duplicates: 0, removed: 0, unknown: 0 });
    await render(ScanGames);
    await userEvent.click(page.getByRole('button', { name: 'Scan games folder' }));
    await expect.element(page.getByText('No game files in the games folder yet.')).toBeInTheDocument();
  });
});
