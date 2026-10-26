import { afterEach, describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { navigate } from 'astro:transitions/client';
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
