import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, ApiError, type ControlsData } from '../../../state/api';
import fixture from '../../../../../crates/brisk-hostd/tests/fixtures/controls/get.json';
import ControlsPanel from './ControlsPanel.svelte';

const data = (over: Partial<ControlsData> = {}) => structuredClone({ ...(fixture as unknown as ControlsData), ...over });
const ID = 0xe0000001;

// A fake pad that tests move by hand; navigator.getGamepads reads it each frame.
let pad: { connected: boolean; down: Record<number, number>; axes: number[] };
const press = (...idx: number[]) => (pad.down = Object.fromEntries(idx.map((i) => [i, 1])));
const release = () => (pad.down = {});
const hs = (id: string) => document.querySelector<SVGElement>(`[data-hs="${id}"]`)!;
const tag = (id: string) => hs(id).querySelector('.tg')?.textContent?.trim();
const status = () => document.querySelector('[role="status"].status')?.textContent?.trim() ?? '';

beforeEach(() => {
  pad = { connected: true, down: {}, axes: [0, 0, 0, 0] };
  vi.spyOn(navigator, 'getGamepads').mockImplementation(
    () =>
      [
        pad.connected && {
          index: 0,
          id: '8BitDo Ultimate 2 (STANDARD GAMEPAD Vendor: 2dc8 Product: 6012)',
          connected: true,
          mapping: 'standard',
          buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: (pad.down[i] ?? 0) > 0.5, value: pad.down[i] ?? 0 })),
          axes: pad.axes,
        },
      ] as unknown as Gamepad[],
  );
  vi.spyOn(api, 'controls').mockImplementation(async () => data());
  vi.spyOn(api, 'putControls').mockImplementation(async (_, b) => data({ bindings: b.bindings, deadZone: b.deadZone, rev: 'next' }));
  vi.spyOn(api, 'resetControls').mockImplementation(async () => data({ rev: 'reset' }));
});
afterEach(() => vi.restoreAllMocks());

const open = async (over: Partial<ControlsData> = {}) => {
  vi.mocked(api.controls).mockImplementation(async () => data(over));
  await render(ControlsPanel, { props: { appid: ID, name: 'Mario Kart Wii' } });
  await expect.element(page.getByRole('group', { name: 'Classic Controller' })).toBeInTheDocument();
};
const listen = async (id: string) => {
  hs(id).focus();
  await userEvent.keyboard('{Enter}'); // select
  await userEvent.keyboard('{Enter}'); // listen
  await expect.poll(() => tag(id)).toBe('press...');
  await new Promise((r) => setTimeout(r, 60)); // a frame or two: the baseline
};

describe('ControlsPanel', () => {
  it('draws 14 hotspots with the fixture tags, roles and names', async () => {
    await open();
    expect(document.querySelectorAll('[data-hs]')).toHaveLength(14);
    expect(tag('a')).toBe('B');
    expect(tag('zl')).toBe('LB');
    expect(tag('ls')).toBe('Left stick');
    const a = page.getByRole('button', { name: 'A, bound to B. Press Enter to change.' });
    await expect.element(a).toHaveAttribute('aria-pressed', 'true'); // the first control starts selected
    await expect.element(page.getByRole('button', { name: /^ZL, bound to LB/ })).toHaveAttribute('aria-pressed', 'false');
    for (const el of document.querySelectorAll('[data-hs]')) expect(el.getAttribute('tabindex')).toBe('0');
    await expect.element(page.getByRole('heading', { name: 'Controls' })).toBeInTheDocument();
    await expect.element(page.getByText(/shared by every Dolphin game on this machine/)).toBeInTheDocument();
  });

  it('click selects without listening; the card shows the control and a changed chip', async () => {
    await open({ bindings: { ...data().bindings, zl: 'RB', zr: 'LB' } });
    await userEvent.click(page.getByRole('button', { name: /^ZL, bound to RB/ }));
    await expect.element(page.getByRole('button', { name: /^ZL, bound to RB/ })).toHaveAttribute('aria-pressed', 'true');
    await expect.element(page.getByText('Selected control')).toBeInTheDocument();
    await expect.element(page.getByText('changed')).toBeInTheDocument();
    expect(tag('zl')).toBe('RB');
    await expect.element(page.getByRole('button', { name: 'Listen for a button' })).toBeInTheDocument();
  });

  it('Enter on a selected hotspot listens: tag reads press..., the card and live region say so', async () => {
    await open();
    hs('x').focus();
    await userEvent.keyboard('{Enter}'); // selects
    expect(tag('x')).toBe('Y');
    await userEvent.keyboard('{Enter}'); // listens
    await expect.poll(() => tag('x')).toBe('press...');
    await expect.element(page.getByRole('button', { name: 'X, listening. Press a button on your pad.' })).toBeInTheDocument();
    await expect.element(page.getByText('Press a button on your pad. Esc to cancel.')).toBeInTheDocument();
    await expect.poll(status).toBe('Listening for X. Press a button on your pad.');
  });

  it('a pad press binds, shows the tag and sends the PUT with the rev', async () => {
    await open();
    await listen('a');
    press(0); // Xbox A
    await expect.poll(() => tag('a')).toBe('A');
    release();
    await expect.poll(() => vi.mocked(api.putControls).mock.calls.length).toBe(1);
    const [id, body] = vi.mocked(api.putControls).mock.calls[0];
    expect(id).toBe(ID);
    expect(body.rev).toBe('5b1f0c9a2d7e4a31');
    expect(body.bindings).toMatchObject({ a: 'A', b: 'B' });
    expect(Object.keys(body.bindings)).toHaveLength(14);
    await expect.poll(status).toContain('Saved to the machine. Used the next time the game starts.');
    expect(status()).toContain('Swapped with B');
    expect(tag('b')).toBe('B');
    expect(document.activeElement).toBe(hs('a')); // selection and focus stay
    expect(hs('a').getAttribute('aria-pressed')).toBe('true');
  });

  it('a button held when listening starts is ignored; a plain bind names no swap', async () => {
    await open();
    press(3); // Y held
    hs('a').focus();
    await userEvent.keyboard('{Enter}{Enter}');
    await new Promise((r) => setTimeout(r, 80));
    expect(tag('a')).toBe('press...');
    release();
    await new Promise((r) => setTimeout(r, 60));
    press(8); // View
    await expect.poll(() => tag('a')).toBe('View');
    expect(status()).toMatch(/^Swapped with Minus \(a pad button can only drive one control\)\./);
  });

  it('the wrong kind of press is ignored with a hint and listening continues', async () => {
    await open();
    await listen('a');
    pad.axes = [0.9, 0, 0, 0];
    await expect.poll(status).toBe('That is a stick. A takes a button.');
    expect(tag('a')).toBe('press...');
    expect(api.putControls).not.toHaveBeenCalled();
  });

  it('Esc cancels, returns focus to the hotspot and sends nothing', async () => {
    await open();
    await listen('a');
    (document.querySelector('.btn.ghost') as HTMLElement).focus(); // focus elsewhere inside the section
    await userEvent.keyboard('{Escape}');
    await expect.poll(() => tag('a')).toBe('B');
    expect(document.activeElement).toBe(hs('a'));
    press(0);
    await new Promise((r) => setTimeout(r, 80));
    expect(api.putControls).not.toHaveBeenCalled();
  });

  it('the Cancel button, Tab and a click elsewhere cancel too', async () => {
    await open();
    await listen('a');
    await userEvent.click(page.getByRole('button', { name: 'Cancel' }));
    await expect.poll(() => tag('a')).toBe('B');
    await listen('a');
    await userEvent.keyboard('{Tab}');
    await expect.poll(() => tag('a')).toBe('B');
    await listen('a');
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    await expect.poll(() => tag('a')).toBe('B');
  });

  it('selecting another control while listening cancels the first and starting a new one replaces it', async () => {
    await open();
    await listen('a');
    await userEvent.click(page.getByRole('button', { name: /^X, bound to/ }));
    await expect.poll(() => tag('a')).toBe('B');
    await listen('x');
    expect(tag('a')).toBe('B');
    expect(tag('x')).toBe('press...');
  });

  it('shows "No pad seen" and waits when listening without a pad', async () => {
    pad.connected = false;
    await open();
    await expect.element(page.getByText('No pad seen. Click a button on your pad, or connect one.')).toBeInTheDocument();
    hs('a').focus();
    await userEvent.keyboard('{Enter}{Enter}');
    await expect.poll(status).toBe('Waiting for a pad.');
    await expect.element(page.getByText('Waiting for a pad. Esc to cancel.')).toBeInTheDocument();
    pad.connected = true;
    await expect.element(page.getByText(/^Pad: /)).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 60));
    press(0);
    await expect.poll(() => tag('a')).toBe('A');
  });

  it('the pad chip names the pad', async () => {
    await open();
    await expect.element(page.getByText(/^Pad: 8BitDo/)).toBeInTheDocument();
  });

  it('Reset to Xbox default posts reset and says so', async () => {
    await open({ bindings: { ...data().bindings, a: 'A', b: 'B' } });
    await userEvent.click(page.getByRole('button', { name: 'Reset to Xbox default' }));
    await expect.poll(() => tag('a')).toBe('B');
    expect(api.resetControls).toHaveBeenCalledWith(ID);
    await expect.poll(status).toContain('Reset to the Xbox default.');
  });

  it('the dead zone slider shows only on sticks, updates while dragging and saves on release', async () => {
    await open();
    expect(page.getByRole('slider', { name: 'Dead zone' }).elements()).toHaveLength(0);
    await userEvent.click(page.getByRole('button', { name: /^Left stick, bound to/ }));
    const slider = page.getByRole('slider', { name: 'Dead zone' });
    await expect.element(slider).toBeInTheDocument();
    await expect.element(page.getByText('How far to push before Dolphin sees movement.')).toBeInTheDocument();
    const el = slider.element() as HTMLInputElement;
    el.value = '12';
    el.dispatchEvent(new Event('input', { bubbles: true }));
    await expect.element(page.getByText('12%')).toBeInTheDocument();
    expect(api.putControls).not.toHaveBeenCalled();
    el.dispatchEvent(new Event('change', { bubbles: true }));
    await expect.poll(() => vi.mocked(api.putControls).mock.calls.length).toBe(1);
    expect(vi.mocked(api.putControls).mock.calls[0][1].deadZone).toEqual({ ls: 12, rs: 0 });
    await userEvent.click(page.getByRole('button', { name: /^A, bound to/ }));
    expect(page.getByRole('slider', { name: 'Dead zone' }).elements()).toHaveLength(0);
  });

  it('shows the apply bar only for a running game after a save, and runs quit, get, put, launch in order', async () => {
    const order: string[] = [];
    await open({ running: true });
    expect(document.querySelector('.bar')).toBeNull(); // nothing changed yet
    vi.spyOn(api, 'quit').mockImplementation(async () => (order.push('quit'), { state: 'closed' }));
    vi.mocked(api.controls).mockImplementation(async () => (order.push('get'), data({ running: true })));
    vi.mocked(api.putControls).mockImplementation(
      async (_, b) => (order.push(b.rev ? 'put-with-rev' : 'put'), data({ bindings: b.bindings, running: true })),
    );
    vi.spyOn(api, 'launch').mockImplementation(async () => void order.push('launch'));
    await listen('a');
    press(0);
    await expect.element(page.getByRole('button', { name: 'Restart game to apply' })).toBeInTheDocument();
    expect(document.querySelector('.bar')?.textContent).toContain('Dolphin reads the mapping at start, so restart the game to use it.');
    release();
    order.length = 0;
    await userEvent.click(page.getByRole('button', { name: 'Restart game to apply' }));
    await expect.poll(() => order).toEqual(['quit', 'get', 'put', 'launch']);
    expect(vi.mocked(api.putControls).mock.calls.at(-1)![1].bindings.a).toBe('A');
    await expect.poll(() => document.querySelector('.bar')).toBeNull();
  });

  it('no apply bar when the game is not running', async () => {
    await open();
    await listen('a');
    press(0);
    await expect.poll(status).toContain('Saved to the machine');
    expect(document.querySelector('.bar')).toBeNull();
  });

  it('a failed restart step is named and Try again repeats the flow', async () => {
    await open({ running: true });
    vi.mocked(api.putControls).mockImplementation(async (_, b) => data({ bindings: b.bindings, running: true }));
    vi.spyOn(api, 'quit').mockResolvedValue({ state: 'closed' });
    vi.spyOn(api, 'launch').mockRejectedValueOnce(new ApiError('boom', 500)).mockResolvedValue(undefined);
    await listen('a');
    press(0);
    await expect.element(page.getByRole('button', { name: 'Restart game to apply' })).toBeInTheDocument();
    release();
    await userEvent.click(page.getByRole('button', { name: 'Restart game to apply' }));
    await expect.element(page.getByText('Could not start the game.')).toBeInTheDocument();
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.poll(() => vi.mocked(api.launch).mock.calls.length).toBe(2);
    await expect.poll(() => document.querySelector('.bar')).toBeNull();
  });

  it('renders nothing when hostd answers 404', async () => {
    vi.mocked(api.controls).mockRejectedValue(new ApiError('no controls for this game', 404));
    await render(ControlsPanel, { props: { appid: ID, name: 'x' } });
    await expect.poll(() => document.querySelector('section')).toBeNull();
  });

  it('shows the load error with Try again', async () => {
    vi.mocked(api.controls).mockRejectedValueOnce(new ApiError('boom', 500));
    await render(ControlsPanel, { props: { appid: ID, name: 'x' } });
    await expect.element(page.getByRole('alert')).toHaveTextContent('Could not load the controls.');
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.element(page.getByRole('group', { name: 'Classic Controller' })).toBeInTheDocument();
  });

  it('a failed save shows the old tag, an alert and Try again', async () => {
    await open();
    vi.mocked(api.putControls).mockRejectedValueOnce(new ApiError('could not save controls', 500));
    await listen('a');
    press(0);
    await expect.element(page.getByRole('alert')).toHaveTextContent('Could not save the controls.');
    expect(tag('a')).toBe('B');
    release();
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.poll(() => tag('a')).toBe('A');
  });

  it('a 409 shows the latest and says another device changed it', async () => {
    await open();
    const current = data({ rev: 'theirs', bindings: { ...data().bindings, l: 'RT', r: 'LT' } });
    vi.mocked(api.putControls).mockRejectedValueOnce(new ApiError('changed elsewhere', 409, { error: 'changed elsewhere', current }));
    await listen('a');
    press(0);
    await expect.poll(status).toBe('Another device changed the controls. Showing the latest.');
    expect(tag('a')).toBe('B');
    expect(tag('l')).toBe('RT');
  });

  it('a custom control reads Custom, with its card text, until it is bound', async () => {
    await open({ bindings: { ...data().bindings, dp: null }, custom: { ...data().custom, dp: true } });
    expect(tag('dp')).toBe('Custom');
    await expect.element(page.getByRole('button', { name: 'D-pad, custom. Press Enter to change.' })).toBeInTheDocument();
    await userEvent.click(page.getByRole('button', { name: /^D-pad, custom/ }));
    await expect
      .element(page.getByText('This control was set in Dolphin and brisk cannot show it. Pick a button to replace it.'))
      .toBeInTheDocument();
    await listen('dp');
    press(14);
    await expect.poll(() => tag('dp')).toBe('D-pad');
  });
});
