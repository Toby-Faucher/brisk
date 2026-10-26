import { describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import AddModule from './AddModule.svelte';

describe('Add a module picker', () => {
  it('opens from the card (click and keyboard), lists the modules, closes on Esc and Done, focus back on the card', async () => {
    await render(AddModule);
    const card = page.getByRole('button', { name: 'Add a module' });
    const dialog = page.getByRole('dialog', { name: 'Modules' });

    await userEvent.click(card);
    await expect.element(dialog).toBeInTheDocument();
    await expect.poll(() => dialog.element().contains(document.activeElement)).toBe(true);
    const items = [...dialog.element().querySelectorAll('li')].map((li) => li.textContent);
    expect(items).toContain('MachinesCard on Home');
    expect(items.some((t) => t?.startsWith('Home'))).toBe(false);
    await userEvent.keyboard('{Escape}');
    await expect.element(dialog).not.toBeInTheDocument();
    await expect.element(card).toHaveFocus();

    await userEvent.keyboard('{Enter}');
    await expect.element(dialog).toBeInTheDocument();
    await userEvent.click(page.getByRole('button', { name: 'Done' }));
    await expect.element(dialog).not.toBeInTheDocument();
    await expect.element(card).toHaveFocus();
  });

  it('stays mounted while its exit transition runs, then unmounts', async () => {
    // base.css is not loaded in browser tests (and motion is reduced): this injected rule is the only transition.
    const css = document.head.appendChild(document.createElement('style'));
    css.textContent = '.mp { transition: opacity 2000ms linear; } .mp[data-ending-style] { opacity: 0; }';
    try {
      await render(AddModule);
      await userEvent.click(page.getByRole('button', { name: 'Add a module' }));
      await expect.element(page.getByRole('dialog', { name: 'Modules' })).toBeInTheDocument();
      await userEvent.keyboard('{Escape}');
      await new Promise((r) => setTimeout(r, 100));
      expect(document.querySelector('.mp')?.hasAttribute('data-ending-style')).toBe(true);
      await expect.poll(() => document.querySelector('.mp'), { timeout: 4000 }).toBeNull();
    } finally {
      css.remove();
    }
  });
});
