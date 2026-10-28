import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import classic from '../../../../crates/brisk-hostd/tests/fixtures/controls/v2-classic-get.json';
import gc from '../../../../crates/brisk-hostd/tests/fixtures/controls/v2-gamecube-get.json';
import { api, ApiError, type ControllerType, type ControlsData } from '../../state/api';
import EmulatorMappings from './EmulatorMappings.svelte';
import PadDiagram from './PadDiagram.svelte';
import { padView } from './controllertest';

const fixture = (t: ControllerType, over: Partial<ControlsData> = {}) =>
  structuredClone({ ...((t === 'classic' ? classic : gc) as unknown as ControlsData), ...over });
let over: Partial<Record<ControllerType, Partial<ControlsData>>>;

// A fake pad that tests move by hand; navigator.getGamepads reads it each frame.
let pad: { connected: boolean; down: Record<number, number>; axes: number[] };
const press = (...idx: number[]) => (pad.down = Object.fromEntries(idx.map((i) => [i, 1])));
const release = () => (pad.down = {});
const wait = (ms = 70) => new Promise((r) => setTimeout(r, ms));
const status = () => document.querySelector('[role="status"].status')?.textContent?.trim() ?? '';
const bnd = (name: string) => document.querySelector(`[data-pad="${name}"] .bnd`)?.textContent;
const padEl = (name: string) => document.querySelector<SVGElement>(`[data-pad="${name}"]`)!;
const change = (name: string) => page.getByRole('button', { name: new RegExp(`^${name}, (bound to|custom|listening)`) });
const row = (name: string) => page.getByRole('button', { name, exact: true });
const chipOf = (id: string) => document.querySelector(`[data-change="${id}"]`)!.closest('li')!.querySelector('.bind')!.textContent!.trim();
const putBody = (n = 0) => vi.mocked(api.putEmulatorControls).mock.calls[n];

beforeEach(() => {
  over = {};
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
  vi.spyOn(api, 'emulatorControls').mockImplementation(async (t) => fixture(t, over[t]));
  vi.spyOn(api, 'putEmulatorControls').mockImplementation(async (t, b) =>
    fixture(t, { ...over[t], bindings: b.bindings, deadZone: b.deadZone, rev: 'next' }),
  );
  vi.spyOn(api, 'resetEmulatorControls').mockImplementation(async (t) => fixture(t, { rev: 'reset' }));
});
afterEach(() => vi.restoreAllMocks());

const open = async (o: Partial<ControlsData> = {}) => {
  over.classic = o;
  await render(EmulatorMappings);
  await expect.element(page.getByRole('group', { name: 'Pad buttons' })).toBeInTheDocument();
};
const listen = async (name: string) => {
  await userEvent.click(change(name));
  await expect.element(page.getByRole('button', { name: new RegExp(`^${name}, listening`) })).toBeInTheDocument();
  await wait(); // a frame or two: the baseline
};

describe('EmulatorMappings: drawing and list', () => {
  it('draws the pad in select mode with bound labels from the fixture, a list of 14 controls and no aria-hidden', async () => {
    await open();
    const svg = document.querySelector('svg.pad')!;
    expect(svg.getAttribute('role')).toBe('group');
    expect(svg.hasAttribute('aria-hidden')).toBe(false);
    expect(bnd('B')).toBe('A'); // Classic A is on pad B
    expect(bnd('LB')).toBe('ZL');
    expect(bnd('View')).toBe('−');
    expect(bnd('Left stick')).toBe('Left stick');
    expect(document.querySelectorAll('.item')).toHaveLength(14);
    await expect.element(page.getByRole('heading', { name: 'Emulator mappings' })).toBeInTheDocument();
    await expect.element(page.getByText(/shared by every Dolphin game on this machine/)).toBeInTheDocument();
    await expect.element(page.getByText('Click a button on the pad, or pick a control from the list.')).toBeInTheDocument();
    expect(chipOf('zl')).toBe('LB');
    expect(chipOf('minus')).toBe('Back'); // the drawing's printed name
  });

  it('roles, names and aria-pressed: drawn buttons and rows', async () => {
    await open();
    const lb = page.getByRole('button', { name: 'LB, does ZL. Press Enter to change.' });
    await expect.element(lb).toHaveAttribute('aria-pressed', 'false');
    await expect.element(lb).toHaveAttribute('tabindex', '0');
    await expect.element(page.getByRole('button', { name: 'ZL, bound to LB. Press Enter to change.' })).toBeInTheDocument();
    await userEvent.click(lb);
    await expect.element(lb).toHaveAttribute('aria-pressed', 'true');
    await expect.element(row('ZL')).toHaveAttribute('aria-pressed', 'false');
    // the d-pad cross is one tab stop
    expect([...document.querySelectorAll('[data-pad="D-pad"]')].filter((e) => e.getAttribute('tabindex') === '0')).toHaveLength(1);
  });

  it('the type switch is a radiogroup; arrows and clicks change the list and the labels', async () => {
    await open();
    const group = page.getByRole('radiogroup', { name: 'Controller type' });
    await expect.element(group).toBeInTheDocument();
    const c = page.getByRole('radio', { name: 'Classic Controller' });
    await expect.element(c).toHaveAttribute('aria-checked', 'true');
    (c.element() as HTMLElement).focus();
    await userEvent.keyboard('{ArrowRight}');
    await expect.element(page.getByRole('radio', { name: 'GameCube controller' })).toHaveAttribute('aria-checked', 'true');
    await expect.element(row('Control stick')).toBeInTheDocument();
    expect(document.querySelectorAll('.item')).toHaveLength(11);
    expect(api.emulatorControls).toHaveBeenCalledWith('gamecube');
    await expect.poll(() => bnd('RB')).toBe('Z');
    expect(bnd('B')).toBe('X'); // position based default
    await userEvent.click(c);
    await expect.element(row('ZL')).toBeInTheDocument();
    expect(bnd('LB')).toBe('ZL');
  });

  it('an unbound pad button is dashed, read as not used, and the prompt says it does nothing yet', async () => {
    await open();
    await userEvent.click(page.getByRole('radio', { name: 'GameCube controller' }));
    await expect.poll(() => bnd('RB')).toBe('Z');
    const back = page.getByRole('button', { name: 'Back, not used. Press Enter to choose what it does.' });
    await expect.element(back).toBeInTheDocument();
    expect(back.element().classList.contains('free')).toBe(true);
    expect(padEl('RB').classList.contains('free')).toBe(false);
    await userEvent.click(back);
    await expect.element(page.getByText('Back does nothing yet. Pick what it should do:')).toBeInTheDocument();
    expect(document.querySelectorAll('input[name="em-pick"]')).toHaveLength(8); // the eight button-like controls
    expect(document.querySelectorAll('input[name="em-pick"]:checked')).toHaveLength(0);
  });
});

describe('EmulatorMappings: two ways, one mapping', () => {
  it('pad-first: click LB, the prompt names its job, picking a radio swaps and saves', async () => {
    await open();
    await userEvent.click(page.getByRole('button', { name: /^LB, does ZL/ }));
    const fs = page.getByRole('group', { name: 'LB does ZL. Pick what it should do:' });
    await expect.element(fs).toBeInTheDocument();
    expect(document.querySelectorAll('input[name="em-pick"]')).toHaveLength(11); // buttons only
    expect((page.getByRole('radio', { name: 'ZL', exact: true }).element() as HTMLInputElement).checked).toBe(true);
    await userEvent.click(page.getByRole('radio', { name: 'A', exact: true }));
    await expect.poll(() => vi.mocked(api.putEmulatorControls).mock.calls.length).toBe(1);
    const [t, body] = putBody();
    expect(t).toBe('classic');
    expect(body.rev).toBe('5b1f0c9a2d7e4a31');
    expect(body.bindings).toMatchObject({ a: 'LB', zl: 'B', b: 'A' });
    expect(Object.keys(body.bindings)).toHaveLength(14);
    await expect.poll(status).toContain('Swapped with ZL. A pad button can only do one thing.');
    expect(status()).toContain('Saved. Used the next time a game starts.');
    expect(chipOf('a')).toBe('LB');
    expect(chipOf('zl')).toBe('B');
    expect(bnd('LB')).toBe('A');
  });

  it('pad-first: a stick hotspot offers only the sticks, the cross only the D-pad; Esc closes and returns focus', async () => {
    await open();
    await userEvent.click(page.getByRole('button', { name: /^Left stick, does/ }));
    expect(document.querySelectorAll('input[name="em-pick"]')).toHaveLength(2);
    await userEvent.keyboard('{Escape}');
    expect(document.querySelector('fieldset')).toBeNull();
    await expect.poll(() => document.activeElement).toBe(padEl('Left stick'));
    await userEvent.click(page.getByRole('button', { name: /^D-pad, does/ }).first());
    expect(document.querySelectorAll('input[name="em-pick"]')).toHaveLength(1);
    await userEvent.click(page.getByRole('radio', { name: 'D-pad' })); // the one already checked: closes
    expect(document.querySelector('fieldset')).toBeNull();
    expect(api.putEmulatorControls).not.toHaveBeenCalled();
  });

  it('pad-first works from the keyboard: Enter on the drawn button, arrow in the radios', async () => {
    await open();
    padEl('A').focus();
    await userEvent.keyboard('{Enter}');
    await expect.element(page.getByRole('group', { name: 'A does B. Pick what it should do:' })).toBeInTheDocument();
    (page.getByRole('radio', { name: 'B', exact: true }).element() as HTMLElement).focus();
    await userEvent.keyboard('{ArrowDown}');
    await expect.poll(() => vi.mocked(api.putEmulatorControls).mock.calls.length).toBe(1);
  });

  it('list-first: Change, a pad press binds and saves; selection and focus stay', async () => {
    await open();
    await listen('A');
    await expect.element(page.getByText('Press a button on your pad')).toBeInTheDocument();
    await expect.poll(status).toBe('Listening for A. Press a button on your pad.');
    press(0); // Xbox A
    await expect.poll(() => chipOf('a')).toBe('A');
    release();
    await expect.poll(() => vi.mocked(api.putEmulatorControls).mock.calls.length).toBe(1);
    expect(putBody()[1].bindings).toMatchObject({ a: 'A', b: 'B' });
    await expect.poll(status).toContain('Swapped with B.');
    expect(chipOf('b')).toBe('B');
    expect(document.activeElement).toBe(document.querySelector('[data-change="a"]'));
    expect(document.querySelector('[data-change="a"]')!.closest('li')!.classList.contains('on')).toBe(true);
  });

  it('list-first: a button held at start is ignored, a plain bind names the new pad button', async () => {
    await open({ bindings: { ...classic.bindings, zl: null as never }, custom: { ...classic.custom, zl: true } });
    press(3); // Y held
    await userEvent.click(change('A'));
    await wait(100);
    expect(chipOf('a')).toBe('Press a button on your pad');
    release();
    await wait();
    press(4); // LB, free because zl is custom
    await expect.poll(() => chipOf('a')).toBe('LB');
    await expect.poll(status).toContain('A is now on LB.');
  });

  it('list-first: a click on the drawing answers with no pad at all', async () => {
    pad.connected = false;
    await open();
    await expect.element(page.getByText('No pad seen')).toBeInTheDocument();
    await userEvent.click(change('A'));
    await expect.element(page.getByText('Waiting for a pad. You can also click a button on the drawing.')).toBeInTheDocument();
    await expect.poll(status).toBe('Waiting for a pad.');
    await userEvent.click(page.getByRole('button', { name: /^LB, does ZL/ }));
    await expect.poll(() => chipOf('a')).toBe('LB');
    expect(chipOf('zl')).toBe('B');
    await expect.poll(() => vi.mocked(api.putEmulatorControls).mock.calls.length).toBe(1);
  });

  it('the wrong kind is ignored with a hint and listening goes on', async () => {
    await open();
    await listen('A');
    pad.axes = [0.9, 0, 0, 0];
    await expect.poll(status).toBe('That is a stick. A takes a button.');
    await userEvent.click(page.getByRole('button', { name: /^Left stick, does/ })); // the click path says the same
    expect(chipOf('a')).toBe('Press a button on your pad');
    expect(api.putEmulatorControls).not.toHaveBeenCalled();
  });

  it('Esc, Cancel and a click elsewhere cancel and send nothing; Esc returns focus to Change', async () => {
    await open();
    await listen('A');
    await userEvent.keyboard('{Escape}');
    await expect.element(change('A')).toBeInTheDocument();
    await expect.poll(() => document.activeElement).toBe(document.querySelector('[data-change="a"]'));
    await listen('A');
    await userEvent.click(page.getByRole('button', { name: /^A, listening/ }));
    await expect.element(change('A')).toBeInTheDocument();
    await listen('A');
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    await expect.element(change('A')).toBeInTheDocument();
    press(0);
    await wait(80);
    expect(api.putEmulatorControls).not.toHaveBeenCalled();
  });

  it('starting a new listen replaces the old one', async () => {
    await open();
    await listen('A');
    await userEvent.click(change('X'));
    await expect.element(page.getByRole('button', { name: /^X, listening/ })).toBeInTheDocument();
    expect(page.getByRole('button', { name: /^A, listening/ }).elements()).toHaveLength(0);
  });

  it('clicking a row name selects it and lights its pad button without listening', async () => {
    await open();
    await userEvent.click(row('ZL'));
    await expect.element(row('ZL')).toHaveAttribute('aria-pressed', 'true');
    await expect.element(page.getByRole('button', { name: /^LB, does ZL/ })).toHaveAttribute('aria-pressed', 'true');
    expect(page.getByRole('button', { name: /^ZL, listening/ }).elements()).toHaveLength(0);
  });

  it('a custom control reads Custom with its sentence, and binding replaces it', async () => {
    await open({ bindings: { ...classic.bindings, dp: null as never }, custom: { ...classic.custom, dp: true } });
    expect(chipOf('dp')).toBe('Custom');
    await expect.element(page.getByText('Set in Dolphin. Pick a button to replace it.')).toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'D-pad, custom. Press Enter to change.' })).toBeInTheDocument();
    await listen('D-pad');
    press(14);
    await expect.poll(() => chipOf('dp')).toBe('D-pad');
    await expect.poll(() => vi.mocked(api.putEmulatorControls).mock.calls.length).toBe(1);
  });

  it('a custom control cannot take a held pad button: refused with a sentence', async () => {
    await open({ bindings: { ...classic.bindings, dp: null as never }, custom: { ...classic.custom, dp: true } });
    // D-pad takes only the D-pad, so use a custom button instead
    await userEvent.click(page.getByRole('button', { name: /^Left stick, does/ }));
    expect(api.putEmulatorControls).not.toHaveBeenCalled();
  });
});

describe('EmulatorMappings: dead zone, reset, restart', () => {
  it('the dead zone slider shows only for sticks (either way), tracks while dragging and saves on release', async () => {
    await open();
    expect(page.getByRole('slider', { name: 'Dead zone' }).elements()).toHaveLength(0);
    await userEvent.click(row('Left stick'));
    const slider = page.getByRole('slider', { name: 'Dead zone' });
    await expect.element(slider).toBeInTheDocument();
    await expect.element(page.getByText('How far to push before Dolphin sees movement.')).toBeInTheDocument();
    const el = slider.element() as HTMLInputElement;
    expect(el.max).toBe('40');
    el.value = '12';
    el.dispatchEvent(new Event('input', { bubbles: true }));
    await expect.element(page.getByText('12%')).toBeInTheDocument();
    expect(api.putEmulatorControls).not.toHaveBeenCalled();
    el.dispatchEvent(new Event('change', { bubbles: true }));
    await expect.poll(() => vi.mocked(api.putEmulatorControls).mock.calls.length).toBe(1);
    expect(putBody()[1].deadZone).toEqual({ ls: 12, rs: 0 });
    await userEvent.click(row('A'));
    expect(page.getByRole('slider', { name: 'Dead zone' }).elements()).toHaveLength(0);
    await userEvent.click(page.getByRole('button', { name: /^Right stick, does/ })); // pad-first also shows it
    await expect.element(page.getByRole('slider', { name: 'Dead zone' })).toBeInTheDocument();
  });

  it('PageUp moves the dead zone by 10 and saves', async () => {
    await open();
    await userEvent.click(row('Left stick'));
    (page.getByRole('slider', { name: 'Dead zone' }).element() as HTMLElement).focus();
    await userEvent.keyboard('{PageUp}');
    await expect.poll(() => vi.mocked(api.putEmulatorControls).mock.calls.length).toBe(1);
    expect(putBody()[1].deadZone.ls).toBe(10);
  });

  it('Reset to default posts reset and says so', async () => {
    await open({ bindings: { ...classic.bindings, a: 'A', b: 'B' } });
    await userEvent.click(page.getByRole('button', { name: 'Reset to default' }));
    await expect.poll(() => chipOf('a')).toBe('B');
    expect(api.resetEmulatorControls).toHaveBeenCalledWith('classic');
    await expect.poll(status).toContain('Reset to the default.');
  });

  const gameList = (running: boolean) =>
    vi
      .spyOn(api, 'games')
      .mockResolvedValue({ ownedAvailable: true, games: [{ appid: 0xe0000001, name: 'Mario Kart Wii', running } as never] });

  it('the restart bar shows only for a running Dolphin after a save, and runs quit, get, put, launch', async () => {
    gameList(true);
    const order: string[] = [];
    await open({ running: true });
    expect(document.querySelector('.bar')).toBeNull();
    vi.spyOn(api, 'quit').mockImplementation(async () => (order.push('quit'), { state: 'closed' }));
    vi.mocked(api.emulatorControls).mockImplementation(async (t) => (order.push('get'), fixture(t, { running: true })));
    vi.mocked(api.putEmulatorControls).mockImplementation(
      async (t, b) => (order.push(b.rev ? 'put-with-rev' : 'put'), fixture(t, { bindings: b.bindings, running: true })),
    );
    vi.spyOn(api, 'launch').mockImplementation(async () => void order.push('launch'));
    await userEvent.click(page.getByRole('button', { name: /^LB, does ZL/ }));
    await userEvent.click(page.getByRole('radio', { name: 'A', exact: true }));
    await expect.element(page.getByRole('button', { name: 'Restart game to apply' })).toBeInTheDocument();
    expect(document.querySelector('.bar')?.textContent).toContain('Dolphin reads the mapping at start, so restart the game to use it.');
    order.length = 0;
    await userEvent.click(page.getByRole('button', { name: 'Restart game to apply' }));
    await expect.poll(() => order).toEqual(['quit', 'get', 'put', 'launch']);
    expect(vi.mocked(api.launch)).toHaveBeenCalledWith(0xe0000001);
    await expect.poll(() => document.querySelector('.bar')).toBeNull();
  });

  it('with no running game to restart the bar has only the text', async () => {
    gameList(false);
    await open({ running: true });
    await userEvent.click(page.getByRole('button', { name: /^LB, does ZL/ }));
    await userEvent.click(page.getByRole('radio', { name: 'A', exact: true }));
    await expect.poll(() => document.querySelector('.bar')?.textContent ?? '').toContain('restart the game to use it');
    await wait(50);
    expect(page.getByRole('button', { name: 'Restart game to apply' }).elements()).toHaveLength(0);
  });

  it('no restart bar when Dolphin is not running', async () => {
    await open();
    await userEvent.click(page.getByRole('button', { name: /^LB, does ZL/ }));
    await userEvent.click(page.getByRole('radio', { name: 'A', exact: true }));
    await expect.poll(status).toContain('Saved.');
    expect(document.querySelector('.bar')).toBeNull();
  });
});

describe('EmulatorMappings: states', () => {
  it('a 404 shows a short "no Dolphin" message instead of the card', async () => {
    vi.mocked(api.emulatorControls).mockRejectedValue(new ApiError('no controls on this machine', 404));
    await render(EmulatorMappings);
    await expect.element(page.getByText(/No Dolphin was found on this machine/)).toBeInTheDocument();
    expect(document.querySelector('.pad')).toBeNull();
  });

  it('shows the load error with Try again', async () => {
    vi.mocked(api.emulatorControls).mockRejectedValueOnce(new ApiError('boom', 500));
    await render(EmulatorMappings);
    await expect.element(page.getByRole('alert')).toHaveTextContent('Could not load the mapping.');
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.element(page.getByRole('group', { name: 'Pad buttons' })).toBeInTheDocument();
  });

  it('loading shows a busy card with the intro', async () => {
    let ok!: (v: ControlsData) => void;
    vi.mocked(api.emulatorControls).mockReturnValue(new Promise((r) => (ok = r)));
    await render(EmulatorMappings);
    await expect.element(page.getByText('Loading the mapping...')).toBeInTheDocument();
    expect(document.querySelector('section')!.getAttribute('aria-busy')).toBe('true');
    ok(fixture('classic'));
    await expect.element(page.getByRole('group', { name: 'Pad buttons' })).toBeInTheDocument();
  });

  it('a failed save puts the old chip back, alerts, and Try again saves', async () => {
    await open();
    vi.mocked(api.putEmulatorControls).mockRejectedValueOnce(new ApiError('could not save controls', 500));
    await userEvent.click(page.getByRole('button', { name: /^LB, does ZL/ }));
    await userEvent.click(page.getByRole('radio', { name: 'A', exact: true }));
    await expect.element(page.getByRole('alert')).toHaveTextContent('Could not save the mapping.');
    expect(chipOf('a')).toBe('B');
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.poll(() => chipOf('a')).toBe('LB');
  });

  it('a 409 shows the latest and says another device changed it', async () => {
    await open();
    const current = fixture('classic', { rev: 'theirs', bindings: { ...classic.bindings, l: 'RT', r: 'LT' } });
    vi.mocked(api.putEmulatorControls).mockRejectedValueOnce(
      new ApiError('changed elsewhere', 409, { error: 'changed elsewhere', current }),
    );
    await userEvent.click(page.getByRole('button', { name: /^LB, does ZL/ }));
    await userEvent.click(page.getByRole('radio', { name: 'A', exact: true }));
    await expect.poll(status).toContain('Another device changed the mapping. Showing the latest.');
    expect(chipOf('a')).toBe('B');
    expect(chipOf('l')).toBe('RT');
  });

  it('GameCube with portReady false shows the port note', async () => {
    await open();
    await userEvent.click(page.getByRole('radio', { name: 'GameCube controller' }));
    await expect.element(page.getByText('Port 1 is not set to a standard controller. Saving a mapping fixes that.')).toBeInTheDocument();
    await expect.element(page.getByText('Using the default mapping. The first change creates the file.')).toBeInTheDocument();
  });

  it('GameCube PUT goes to the gamecube route with its own ids', async () => {
    await open();
    await userEvent.click(page.getByRole('radio', { name: 'GameCube controller' }));
    await expect.element(row('Control stick')).toBeInTheDocument();
    await userEvent.click(row('Control stick'));
    (page.getByRole('slider', { name: 'Dead zone' }).element() as HTMLElement).focus();
    await userEvent.keyboard('{PageUp}');
    await expect.poll(() => vi.mocked(api.putEmulatorControls).mock.calls.length).toBe(1);
    expect(putBody()[0]).toBe('gamecube');
    expect(putBody()[1].deadZone).toEqual({ ms: 10, cs: 0 });
    expect(Object.keys(putBody()[1].bindings)).toHaveLength(11);
  });

  it('the pad chip names the pad, or says No pad seen', async () => {
    await open();
    await expect.element(page.getByText(/^Pad: 8BitDo/)).toBeInTheDocument();
    pad.connected = false;
    await expect.element(page.getByText('No pad seen')).toBeInTheDocument();
  });

  it('a pressed physical button lights its drawn twin', async () => {
    await open();
    press(4);
    await expect.poll(() => padEl('LB').classList.contains('on')).toBe(true);
  });
});

describe('PadDiagram', () => {
  const view = padView({ index: 0, id: 'p', connected: true, mapping: 'standard', buttons: [], axes: [] }, 0);
  it('without select it is aria-hidden, has no extra roles and no hit areas', async () => {
    await render(PadDiagram, { props: { view, dz: 0, trails: ['', ''] } });
    const svg = document.querySelector('svg.pad')!;
    expect(svg.getAttribute('aria-hidden')).toBe('true');
    expect(svg.hasAttribute('role')).toBe(false);
    expect(svg.querySelectorAll('[role], [tabindex], .hit, .bnd')).toHaveLength(0);
    expect(document.querySelector('figcaption')?.textContent).toContain('8BitDo Ultimate 2 to scale');
  });

  it('with select: pick fires on click and on Enter and Space; selected and the whole cross highlight', async () => {
    const onpick = vi.fn();
    await render(PadDiagram, {
      props: { view, dz: 0, trails: ['', ''], select: { current: 'D-pad', onpick, labels: { A: 'B' }, hint: 'Pick one' } },
    });
    expect(document.querySelectorAll('[role="button"]')).toHaveLength(17);
    expect(document.querySelectorAll('.sel')).toHaveLength(4);
    expect(document.querySelector('[data-pad="A"]')!.getAttribute('aria-pressed')).toBe('false');
    expect(document.querySelector('figcaption')?.textContent).toBe('Pick one');
    await userEvent.click(page.getByRole('button', { name: /^A, does B/ }));
    (document.querySelector('[data-pad="X"]') as HTMLElement).focus();
    await userEvent.keyboard('{Enter}');
    await userEvent.keyboard(' ');
    expect(onpick.mock.calls.map((c) => c[0])).toEqual(['A', 'X', 'X']);
  });

  it('two instances on one page share no ids and each keeps its own drawing', async () => {
    await render(PadDiagram, { props: { view, dz: 0, trails: ['', ''] } });
    await render(PadDiagram, { props: { view, dz: 0, trails: ['', ''], select: { current: null, onpick: vi.fn(), labels: {} } } });
    const ids = [...document.querySelectorAll('[id]')].map((e) => e.id);
    expect(ids.length).toBeGreaterThan(40);
    expect(new Set(ids).size).toBe(ids.length);
    const svgs = document.querySelectorAll('svg.pad');
    expect(svgs).toHaveLength(2);
    for (const svg of svgs) {
      expect(svg.querySelector('[id$="-pad-body"]')).not.toBeNull();
      expect(svg.querySelectorAll('text.t').length).toBeGreaterThan(15);
    }
  });
});
