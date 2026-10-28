import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import Dropdown from './Dropdown.svelte';
import DropdownInDialog from './DropdownInDialog.fixture.svelte';

const options: [string, string][] = [['default', 'Default order'], ['az', 'A-Z'], ['most', 'Most played']];

describe('Dropdown', () => {
  it('a labelled button showing the choice; opens a rounded listbox; keys pick; Escape closes back to the button', async () => {
    const onchange = vi.fn();
    await render(Dropdown, { props: { value: 'default', options, label: 'Sort games', onchange } });
    const trigger = page.getByRole('combobox', { name: 'Sort games' });
    await expect.element(trigger).toHaveTextContent('Default order');
    await userEvent.click(trigger);
    const list = page.getByRole('listbox');
    await expect.element(list).toBeInTheDocument();
    expect(list.getByRole('option').elements()).toHaveLength(3);
    await expect.element(list.getByRole('option', { name: 'Default order' })).toHaveAttribute('aria-selected', 'true');
    expect(parseFloat(getComputedStyle(document.querySelector('.dd-menu')!).borderTopLeftRadius)).toBeGreaterThanOrEqual(12);
    await userEvent.keyboard('{ArrowDown}{Enter}');
    await expect.element(list).not.toBeInTheDocument();
    expect(onchange).toHaveBeenCalledExactlyOnceWith('az');
    await expect.element(trigger).toHaveTextContent('A-Z');
    await expect.poll(() => document.activeElement).toBe(trigger.element());
    // keyboard only: open from the focused button, Escape changes nothing
    await userEvent.keyboard('{ArrowDown}');
    await expect.element(page.getByRole('listbox')).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    await expect.element(page.getByRole('listbox')).not.toBeInTheDocument();
    expect(onchange).toHaveBeenCalledTimes(1);
    await expect.poll(() => document.activeElement).toBe(trigger.element());
  });

  it('the chevron sits a few px in from the right edge', async () => {
    await render(Dropdown, { props: { value: 'az', options, label: 'Sort games' } });
    const t = page.getByRole('combobox', { name: 'Sort games' }).element();
    const gap = t.getBoundingClientRect().right - t.querySelector('svg')!.getBoundingClientRect().right;
    expect(gap).toBeGreaterThanOrEqual(10);
    expect(gap).toBeLessThanOrEqual(16);
  });
});

describe('Dropdown inside a Dialog', () => {
  it('the menu is the topmost thing at its own spot, picks by click, and Escape closes only the menu', async () => {
    await render(DropdownInDialog);
    const trigger = page.getByRole('combobox', { name: 'Sort games' });
    await userEvent.click(trigger);
    const opt = page.getByRole('option', { name: 'Beta' });
    await expect.element(opt).toBeInTheDocument();
    const r = opt.element().getBoundingClientRect();
    expect(opt.element().contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2))).toBe(true);
    await userEvent.keyboard('{Escape}');
    await expect.element(page.getByRole('listbox')).not.toBeInTheDocument();
    await expect.element(page.getByRole('dialog')).toBeInTheDocument();
    await userEvent.click(trigger);
    await userEvent.click(page.getByRole('option', { name: 'Beta' }));
    await expect.element(trigger).toHaveTextContent('Beta');
  });
});
