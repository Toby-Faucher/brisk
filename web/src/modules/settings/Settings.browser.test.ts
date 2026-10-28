import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import Settings from './Settings.svelte';

describe('Settings tabs', () => {
  it('arrow keys, Home and End move the tab and write the hash', async () => {
    history.replaceState(null, '', '#general');
    await render(Settings);
    const tab = (name: string) => page.getByRole('tab', { name });
    await expect.element(tab('General')).toHaveAttribute('aria-selected', 'true');
    await userEvent.click(tab('General'));

    const steps: [string, string, string][] = [
      ['{ArrowRight}', 'Streaming', '#streaming'],
      ['{ArrowRight}', 'Controller', '#controller'],
      ['{ArrowRight}', 'Emulators', '#emulators'],
      ['{ArrowLeft}', 'Controller', '#controller'],
      ['{ArrowLeft}', 'Streaming', '#streaming'],
      ['{End}', 'About', '#about'],
      ['{Home}', 'General', '#general'],
    ];
    for (const [key, name, hash] of steps) {
      await userEvent.keyboard(key);
      await expect.element(tab(name)).toHaveAttribute('aria-selected', 'true');
      await expect.element(tab(name)).toHaveFocus();
      expect(location.hash, key).toBe(hash);
    }
  });

  it('#emulators/dolphin/folders shows the Games folders section with the scan button', async () => {
    history.replaceState(null, '', '#emulators/dolphin/folders');
    await render(Settings);
    await expect.element(page.getByRole('tab', { name: 'Games folders' })).toHaveAttribute('aria-selected', 'true');
    await expect.element(page.getByRole('button', { name: 'Scan games folder' })).toBeInTheDocument();
    await expect.element(page.getByText(/Removing a game from the Library never deletes its file/)).toBeInTheDocument();
  });

  const mock = async () => {
    const { api } = await import('../../state/api');
    const fx = (await import('../../../../crates/brisk-hostd/tests/fixtures/controls/v2-classic-get.json')).default;
    return vi.spyOn(api, 'emulatorControls').mockResolvedValue(fx as never);
  };

  it('the Controller tab keeps the tester and points to Emulators, with no mapping card', async () => {
    history.replaceState(null, '', '#controller');
    await render(Settings);
    await expect.element(page.getByRole('link', { name: 'Emulators' })).toHaveAttribute('href', '#emulators/dolphin/controllers');
    expect(page.getByRole('heading', { name: 'Emulator mappings' }).elements()).toHaveLength(0);
  });

  it('#emulators/dolphin/controllers opens the pickers and the mappings card, and focuses it', async () => {
    await mock();
    history.replaceState(null, '', '#emulators/dolphin/controllers');
    await render(Settings);
    await expect.element(page.getByRole('tab', { name: 'Emulators' })).toHaveAttribute('aria-selected', 'true');
    await expect.element(page.getByRole('tab', { name: 'Dolphin' })).toHaveAttribute('aria-selected', 'true');
    await expect.element(page.getByRole('tab', { name: 'Controllers' })).toHaveAttribute('aria-selected', 'true');
    expect(page.getByRole('tab', { name: 'Launch options' }).elements()).toHaveLength(0); // planned sections stay hidden
    await expect.element(page.getByRole('heading', { name: 'Emulator mappings' })).toBeInTheDocument();
    await expect.poll(() => document.activeElement?.id).toBe('em-h');
    vi.restoreAllMocks();
  });

  it('the old #controller/dolphin link is rewritten and opens the Emulators tab', async () => {
    await mock();
    history.replaceState(null, '', '#controller/dolphin');
    await render(Settings);
    expect(location.hash).toBe('#emulators/dolphin/controllers');
    await expect.element(page.getByRole('tab', { name: 'Emulators' })).toHaveAttribute('aria-selected', 'true');
    await expect.element(page.getByRole('heading', { name: 'Emulator mappings' })).toBeInTheDocument();
    vi.restoreAllMocks();
  });

  it('bare #emulators shows Dolphin Controllers; a 404 shows the empty message', async () => {
    const { api, ApiError } = await import('../../state/api');
    vi.spyOn(api, 'emulatorControls').mockRejectedValue(new ApiError('no dolphin', 404));
    history.replaceState(null, '', '#emulators');
    await render(Settings);
    await expect.element(page.getByRole('tab', { name: 'Controllers' })).toHaveAttribute('aria-selected', 'true');
    await expect.element(page.getByText(/No Dolphin was found/)).toBeInTheDocument();
    vi.restoreAllMocks();
  });
});
