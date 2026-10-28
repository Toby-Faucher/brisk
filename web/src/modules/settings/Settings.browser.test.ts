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

  it('the Controller tab shows the Emulator mappings card, and #controller/dolphin opens it', async () => {
    const { api } = await import('../../state/api');
    const fx = (await import('../../../../crates/brisk-hostd/tests/fixtures/controls/v2-classic-get.json')).default;
    vi.spyOn(api, 'emulatorControls').mockResolvedValue(fx as never);
    history.replaceState(null, '', '#controller/dolphin');
    await render(Settings);
    await expect.element(page.getByRole('heading', { name: 'Emulator mappings' })).toBeInTheDocument();
    await expect.element(page.getByRole('tab', { name: 'Controller' })).toHaveAttribute('aria-selected', 'true');
    await expect.poll(() => document.activeElement?.id).toBe('em-h');
    vi.restoreAllMocks();
  });
});
