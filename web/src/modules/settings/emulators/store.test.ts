import { describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { ApiError, type ControlsData } from '../../../state/api';
import fixture from '../../../../../crates/brisk-hostd/tests/fixtures/controls/v2-classic-get.json';
import gcFixture from '../../../../../crates/brisk-hostd/tests/fixtures/controls/v2-gamecube-get.json';
import put from '../../../../../crates/brisk-hostd/tests/fixtures/controls/v2-classic-put.json';
import { bind } from './mapping';
import { CONFLICT, createMappings, LOAD_FAILED, SAVE_FAILED, SAVED } from './store';

const data = (over: Partial<ControlsData> = {}) => structuredClone({ ...(fixture as unknown as ControlsData), ...over });
const T = { key: 'emu:dolphin:mario-kart-wii', name: 'Mario Kart Wii' };
const later = <T>() => {
  let ok!: (v: T) => void;
  let no!: (e: unknown) => void;
  const p = new Promise<T>((a, b) => ((ok = a), (no = b)));
  return { p, ok, no };
};

function setup(over: Partial<ReturnType<typeof mocks>> = {}) {
  const m = { ...mocks(), ...over };
  const toast = vi.fn();
  const c = createMappings('classic', { api: m, toast });
  return { c, m, toast, s: () => get(c.state) };
}
function mocks() {
  return {
    emulatorControls: vi.fn(async (_: string) => data()),
    putEmulatorControls: vi.fn(
      async (_: string, b: { rev?: string; bindings: ControlsData['bindings']; deadZone: ControlsData['deadZone'] }) =>
        data({ bindings: b.bindings, deadZone: b.deadZone, rev: `r${Math.random()}` }),
    ),
    resetEmulatorControls: vi.fn(async (_: string) => data({ rev: 'reset' })),
    quit: vi.fn(async (_: string, __?: boolean): Promise<{ state: 'closed' | 'still-running' }> => ({ state: 'closed' })),
    launch: vi.fn(async (_: string) => undefined),
  };
}

describe('load', () => {
  it('shows the fixture', async () => {
    const { c, s, m } = setup();
    expect(s().load).toBe('loading');
    await c.load();
    expect(s().load).toBe('ready');
    expect(s().draft?.bindings.a).toBe('B');
    expect(m.emulatorControls).toHaveBeenCalledWith('classic');
  });
  it('a 404 hides the card; another error shows the error state, and Try again loads', async () => {
    const a = setup({ emulatorControls: vi.fn().mockRejectedValue(new ApiError('no controls for this game', 404)) });
    await a.c.load();
    expect(a.s().load).toBe('hidden');
    const b = setup({ emulatorControls: vi.fn().mockRejectedValueOnce(new ApiError('boom', 500)).mockResolvedValue(data()) });
    await b.c.load();
    expect(b.s()).toMatchObject({ load: 'error', note: { text: LOAD_FAILED, tone: 'error' } });
    await b.c.retry();
    expect(b.s().load).toBe('ready');
  });
});

describe('autosave', () => {
  it('sends the full body with the rev, shows the edit at once, then the saved line', async () => {
    const { c, s, m } = setup();
    await c.load();
    c.setBindings(bind('classic', s().draft!.bindings, 'a', 'A').bindings, null);
    expect(s().draft?.bindings).toMatchObject({ a: 'A', b: 'B' }); // the swap, before the answer
    await c.idle();
    const body = m.putEmulatorControls.mock.calls[0][1];
    expect(body.rev).toBe(put.rev);
    expect(body.bindings).toMatchObject({ a: 'A', b: 'B' });
    expect(s()).toMatchObject({ saving: false, saved: true, changed: true });
    expect(SAVED).toBe('Saved. Used the next time a game starts.');
  });

  it('keeps one save in flight and sends only the latest draft after it, with the new rev', async () => {
    const first = later<ControlsData>();
    const { c, s, m } = setup();
    m.putEmulatorControls.mockImplementationOnce(() => first.p);
    await c.load();
    c.setBindings(bind('classic', s().draft!.bindings, 'a', 'A').bindings, null);
    c.setDeadZone('ls', 5);
    c.setDeadZone('ls', 9);
    expect(m.putEmulatorControls).toHaveBeenCalledTimes(1);
    first.ok(data({ rev: 'second', bindings: { ...data().bindings, a: 'A', b: 'B' } }));
    await c.idle();
    expect(m.putEmulatorControls).toHaveBeenCalledTimes(2);
    expect(m.putEmulatorControls.mock.calls[1][1]).toMatchObject({ rev: 'second', deadZone: { ls: 9 } });
    expect(s().draft?.deadZone.ls).toBe(9);
  });

  it('a failed save puts the old tags back and offers Try again', async () => {
    const { c, s, m } = setup();
    await c.load();
    m.putEmulatorControls.mockRejectedValueOnce(new ApiError('could not save controls', 500));
    c.setBindings(bind('classic', s().draft!.bindings, 'a', 'A').bindings, null);
    await c.idle();
    expect(s().draft?.bindings.a).toBe('B');
    expect(s().note).toEqual({ text: SAVE_FAILED, tone: 'error' });
    await c.retry();
    await c.idle();
    expect(s().draft?.bindings.a).toBe('A');
    expect(s().saved).toBe(true);
  });

  it('a 409 takes the current state from the body without another GET', async () => {
    const { c, s, m } = setup();
    await c.load();
    const current = data({ rev: 'theirs', bindings: { ...data().bindings, x: 'X', y: 'Y' } });
    m.putEmulatorControls.mockRejectedValueOnce(new ApiError('changed elsewhere', 409, { error: 'changed elsewhere', current }));
    c.setDeadZone('rs', 3);
    await c.idle();
    expect(s().note).toEqual({ text: CONFLICT, tone: 'info' });
    expect(s().draft?.bindings.x).toBe('X');
    expect(s().draft?.deadZone.rs).toBe(0);
    expect(m.emulatorControls).toHaveBeenCalledTimes(1);
    // without a body it asks again
    m.putEmulatorControls.mockRejectedValueOnce(new ApiError('changed elsewhere', 409));
    c.setDeadZone('rs', 4);
    await c.idle();
    expect(m.emulatorControls).toHaveBeenCalledTimes(2);
  });

  it('refresh shows the machine unless an edit is on its way', async () => {
    const { c, s, m } = setup();
    await c.load();
    m.emulatorControls.mockResolvedValue(data({ rev: 'x', bindings: { ...data().bindings, l: 'RT', r: 'LT' } }));
    await c.refresh();
    expect(s().draft?.bindings.l).toBe('RT');
    const hold = later<ControlsData>();
    m.putEmulatorControls.mockImplementationOnce(() => hold.p);
    c.setDeadZone('ls', 2);
    m.emulatorControls.mockClear();
    await c.refresh();
    expect(m.emulatorControls).not.toHaveBeenCalled();
    hold.ok(data());
    await c.idle();
  });
});

describe('reset', () => {
  it('posts reset and shows the answer', async () => {
    const { c, s, m } = setup();
    await c.load();
    await c.reset();
    expect(m.resetEmulatorControls).toHaveBeenCalledOnce();
    expect(s()).toMatchObject({ saved: true, note: { text: 'Reset to the default.' } });
    expect(s().data?.rev).toBe('reset');
  });
});

describe('restart', () => {
  async function ready() {
    const t = setup();
    const order: string[] = [];
    t.m.quit.mockImplementation(async () => (order.push('quit'), { state: 'closed' as const }));
    t.m.emulatorControls.mockImplementation(async () => (order.push('get'), data()));
    t.m.putEmulatorControls.mockImplementation(
      async (_, b) => (order.push(b.rev === undefined ? 'put' : 'put-with-rev'), data({ bindings: b.bindings })),
    );
    t.m.launch.mockImplementation(async () => void order.push('launch'));
    await t.c.load();
    order.length = 0;
    t.c.setBindings(bind('classic', t.s().draft!.bindings, 'a', 'A').bindings, null);
    await t.c.idle();
    order.length = 0;
    return { ...t, order };
  }

  it('runs quit, get, put without rev, launch in that order', async () => {
    const { c, s, order, m } = await ready();
    await c.restart(T);
    expect(order).toEqual(['quit', 'get', 'put', 'launch']);
    expect(m.putEmulatorControls.mock.calls.at(-1)![1].bindings.a).toBe('A');
    expect(s()).toMatchObject({ restarting: false, changed: false, restartError: null });
  });

  it('a 409 from quit (already closed) goes on', async () => {
    const { c, order, m } = await ready();
    m.quit.mockRejectedValue(new ApiError('not running', 409));
    await c.restart(T);
    expect(order).toEqual(['get', 'put', 'launch']);
  });

  it('still-running stops with the Force quit toast, and Force quit carries on', async () => {
    const { c, s, order, m, toast } = await ready();
    m.quit.mockResolvedValueOnce({ state: 'still-running' });
    await c.restart(T);
    expect(order).toEqual([]);
    expect(s().restartError).toBe('Could not close the game');
    const opts = toast.mock.calls[0][1];
    expect(toast.mock.calls[0][0]).toBe('Still running: Mario Kart Wii (try Force quit)');
    opts.action.run();
    await vi.waitFor(() => expect(order).toEqual(['quit', 'get', 'put', 'launch']));
    expect(m.quit).toHaveBeenLastCalledWith('emu:dolphin:mario-kart-wii', true);
  });

  it('names the step that failed and Try again repeats the flow', async () => {
    const a = await ready();
    a.m.quit.mockRejectedValueOnce(new ApiError('boom', 500));
    await a.c.restart(T);
    expect(a.s().restartError).toBe('Could not close the game');
    expect(a.order).toEqual([]);

    const b = await ready();
    b.m.putEmulatorControls.mockRejectedValueOnce(new ApiError('boom', 500));
    await b.c.restart(T);
    expect(b.s().restartError).toBe('Could not save the mapping');
    expect(b.order).toEqual(['quit', 'get']);

    const d = await ready();
    d.m.launch.mockRejectedValueOnce(new ApiError('boom', 500));
    await d.c.restart(T);
    expect(d.s().restartError).toBe('Could not start the game');
    d.order.length = 0;
    await d.c.restart(T);
    expect(d.order).toEqual(['quit', 'get', 'put', 'launch']);
    expect(d.s().restartError).toBeNull();
  });

  it('a still-custom control goes as null, a now-plain one keeps the file value', async () => {
    const { c, m, s } = await ready();
    const custom = { ...data().custom, dp: true };
    m.emulatorControls.mockResolvedValue(data({ bindings: { ...data().bindings, dp: null }, custom }));
    c.setBindings({ ...s().draft!.bindings, dp: null, ls: 'Left stick' }, null);
    await c.idle();
    await c.restart(T);
    expect(m.putEmulatorControls.mock.calls.at(-1)![1].bindings.dp).toBeNull();
  });
});

describe('per type', () => {
  it('a GameCube store asks the gamecube route and keeps portReady', async () => {
    const m = { ...mocks(), emulatorControls: vi.fn(async (_: string) => structuredClone(gcFixture) as unknown as ControlsData) };
    const c = createMappings('gamecube', { api: m, toast: vi.fn() });
    await c.load();
    expect(m.emulatorControls).toHaveBeenCalledWith('gamecube');
    expect(get(c.state).data?.portReady).toBe(false);
    c.setDeadZone('ms', 7);
    await c.idle();
    expect(m.putEmulatorControls).toHaveBeenCalledWith('gamecube', expect.objectContaining({ deadZone: { ms: 7, cs: 0 } }));
  });
});
