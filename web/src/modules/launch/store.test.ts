import { get } from 'svelte/store';
import { describe, expect, it, vi } from 'vitest';
import { ApiError, type GameLaunch, type LaunchDefaults, type LaunchOpts } from '../../state/api';
import { emptyForm } from './form';
import { BACK_TO_DEFAULTS, CONFLICT, createDefaults, createGameLaunch, LOAD_FAILED, SAVE_FAILED } from './store';

const none: LaunchOpts = { wrappers: [], env: {}, custom: null, extra_args: [] };
const defs = (o: Partial<LaunchDefaults> = {}): LaunchDefaults => ({
  rev: 'r1',
  allowCustomCommands: false,
  options: none,
  envAllowed: ['DXVK_HUD', 'MANGOHUD'],
  ...o,
});
const game = (o: Partial<GameLaunch> = {}): GameLaunch => ({
  id: 'steam:620',
  rev: 'g1',
  override: null,
  effective: none,
  problems: [],
  allowCustomCommands: false,
  steamLine: '/x/brisk-hostd wrap steam:620 -- %command%',
  ...o,
});
const later = <T>() => {
  let ok!: (v: T) => void;
  const p = new Promise<T>((a) => (ok = a));
  return { p, ok };
};

function setupDefaults(over: Record<string, unknown> = {}) {
  const api = {
    launchDefaults: vi.fn(async () => defs()),
    putLaunchDefaults: vi.fn(async (b: { allowCustomCommands: boolean; options: LaunchOpts }) =>
      defs({ rev: 'r2', allowCustomCommands: b.allowCustomCommands, options: b.options }),
    ),
    ...over,
  };
  const c = createDefaults({ api: api as never, debounce: 0 });
  return { c, api, s: () => get(c.state) };
}

describe('defaults store', () => {
  it('loads, then a failed load can be retried', async () => {
    const { c, s, api } = setupDefaults({
      launchDefaults: vi.fn().mockRejectedValueOnce(new ApiError('x', 500)).mockResolvedValue(defs()),
    });
    expect(s().load).toBe('loading');
    await c.load();
    expect(s()).toMatchObject({ load: 'error', note: { text: LOAD_FAILED } });
    await c.retry();
    expect(s().load).toBe('ready');
    expect(api.launchDefaults).toHaveBeenCalledTimes(2);
  });

  it('saves an edit with the rev, and keeps the draft', async () => {
    const { c, s, api } = setupDefaults();
    await c.load();
    c.edit({ form: { ...emptyForm(), mango: true }, allow: false }, true);
    await c.flush();
    expect(api.putLaunchDefaults).toHaveBeenCalledWith({
      rev: 'r1',
      allowCustomCommands: false,
      options: { ...none, wrappers: ['mangohud'] },
    });
    expect(s()).toMatchObject({ saved: true, saving: false });
    expect(s().server?.rev).toBe('r2');
    expect(s().draft?.form.mango).toBe(true);
  });

  it('one write at a time: an edit during a save follows it, with the new rev, and the latest wins', async () => {
    const first = later<LaunchDefaults>();
    const put = vi
      .fn()
      .mockReturnValueOnce(first.p)
      .mockImplementation(async () => defs({ rev: 'r3' }));
    const { c, s } = setupDefaults({ putLaunchDefaults: put });
    await c.load();
    c.edit({ form: { ...emptyForm(), mango: true }, allow: false }, true);
    await new Promise((r) => setTimeout(r, 5));
    c.edit({ form: { ...emptyForm(), mango: true, gamemode: true }, allow: false }, true);
    await new Promise((r) => setTimeout(r, 5));
    expect(put).toHaveBeenCalledTimes(1);
    first.ok(defs({ rev: 'r2' }));
    await c.flush();
    expect(put).toHaveBeenCalledTimes(2);
    expect(put.mock.calls[1][0].rev).toBe('r2');
    expect(put.mock.calls[1][0].options.wrappers).toEqual(['mangohud', 'gamemode']);
    expect(s().saved).toBe(true);
  });

  it('typing waits for the pause: edits in a row make one write', async () => {
    const api = { launchDefaults: vi.fn(async () => defs()), putLaunchDefaults: vi.fn(async (_: unknown) => defs({ rev: 'r2' })) };
    const c = createDefaults({ api: api as never, debounce: 30 });
    await c.load();
    for (const v of ['a', 'ab', 'abc']) c.edit({ form: { ...emptyForm(), extra: [v] }, allow: false });
    expect(api.putLaunchDefaults).not.toHaveBeenCalled();
    await new Promise((r) => setTimeout(r, 80));
    await c.flush();
    expect(api.putLaunchDefaults).toHaveBeenCalledTimes(1);
    expect(api.putLaunchDefaults.mock.calls[0][0]).toMatchObject({ options: { extra_args: ['abc'] } });
  });

  it('turning the switch off clears the command in the same save', async () => {
    const on = defs({ allowCustomCommands: true, options: { ...none, custom: ['mytool'] } });
    const { c, s, api } = setupDefaults({ launchDefaults: vi.fn(async () => on) });
    await c.load();
    const d = s().draft!;
    expect(d.form.custom).toEqual(['mytool']);
    // the editor does this through withAllow; even if a draft still held a command, the write clears it
    c.edit({ form: d.form, allow: false }, true);
    await c.flush();
    expect(api.putLaunchDefaults).toHaveBeenCalledTimes(1);
    expect(api.putLaunchDefaults.mock.calls[0][0]).toMatchObject({ allowCustomCommands: false, options: { custom: null } });
    expect(s().draft?.form.custom).toEqual([]);
  });

  it('a 400 shows the reasons and keeps the draft; no automatic retry', async () => {
    const put = vi
      .fn()
      .mockRejectedValue(new ApiError('these launch options are not allowed', 400, { problems: ['gamescope: flag not allowed: -x'] }));
    const { c, s } = setupDefaults({ putLaunchDefaults: put });
    await c.load();
    c.edit({ form: { ...emptyForm(), mango: true }, allow: false }, true);
    await c.flush();
    expect(s().problems).toEqual(['gamescope: flag not allowed: -x']);
    expect(s().draft?.form.mango).toBe(true);
    expect(put).toHaveBeenCalledTimes(1);
  });

  it("a 409 shows the other device's state", async () => {
    const cur = defs({ rev: 'r9', options: { ...none, wrappers: ['gamemode'] } });
    const { c, s } = setupDefaults({
      putLaunchDefaults: vi.fn().mockRejectedValue(new ApiError('changed elsewhere', 409, { error: 'changed elsewhere', current: cur })),
    });
    await c.load();
    c.edit({ form: { ...emptyForm(), mango: true }, allow: false }, true);
    await c.flush();
    expect(s().note).toEqual({ text: CONFLICT, tone: 'info' });
    expect(s().server?.rev).toBe('r9');
    expect(s().draft?.form).toMatchObject({ mango: false, gamemode: true });
  });

  it('another failure says so, and Try again writes the draft again', async () => {
    const put = vi
      .fn()
      .mockRejectedValueOnce(new ApiError('boom', 500))
      .mockResolvedValue(defs({ rev: 'r2' }));
    const { c, s } = setupDefaults({ putLaunchDefaults: put });
    await c.load();
    c.edit({ form: { ...emptyForm(), mango: true }, allow: false }, true);
    await c.flush();
    expect(s().note).toEqual({ text: SAVE_FAILED, tone: 'error' });
    await c.retry();
    expect(put).toHaveBeenCalledTimes(2);
    expect(s().saved).toBe(true);
  });

  it('client problems (a bad gamescope number) are shown and nothing is sent', async () => {
    const { c, s, api } = setupDefaults();
    await c.load();
    c.edit({ form: { ...emptyForm(), gs: { on: true, num: { '-r': '5000' }, flags: {} } }, allow: false }, true);
    await c.flush();
    expect(api.putLaunchDefaults).not.toHaveBeenCalled();
    expect(s().problems[0]).toContain('Refresh rate');
  });
});

describe('game store', () => {
  function setup(over: Record<string, unknown> = {}) {
    const api = {
      launchDefaults: vi.fn(async () => defs({ options: { ...none, wrappers: ['mangohud'] } })),
      gameLaunch: vi.fn(async () => game({ effective: { ...none, wrappers: ['mangohud'] } })),
      putGameLaunch: vi.fn(async (_id: string, b: { override: object }) =>
        game({ rev: 'g2', override: b.override, effective: { ...none, wrappers: ['gamemode'] } }),
      ),
      resetGameLaunch: vi.fn(async () => game({ rev: 'g0' })),
      ...over,
    };
    const c = createGameLaunch('steam:620', { api: api as never, debounce: 0 });
    return { c, api, s: () => get(c.state) };
  }

  it('loads the game and the allowed names; fields follow the default until overridden', async () => {
    const { c, s } = setup();
    await c.load();
    expect(s().server?.envAllowed).toEqual(['DXVK_HUD', 'MANGOHUD']);
    expect(s().draft?.inherit).toEqual({ wrappers: true, env: true, custom: true, extra: true });
    expect(s().draft?.form.mango).toBe(true);
  });

  it('still loads when the defaults route fails (no env names)', async () => {
    const { c, s } = setup({ launchDefaults: vi.fn().mockRejectedValue(new ApiError('x', 500)) });
    await c.load();
    expect(s().load).toBe('ready');
    expect(s().server?.envAllowed).toEqual([]);
  });

  it('sends only the overridden fields', async () => {
    const { c, s, api } = setup();
    await c.load();
    const d = s().draft!;
    c.edit({ form: { ...d.form, gamemode: true, mango: false }, inherit: { ...d.inherit, wrappers: false } }, true);
    await c.flush();
    expect(api.putGameLaunch).toHaveBeenCalledWith('steam:620', { rev: 'g1', override: { wrappers: ['gamemode'] } });
  });

  it('all fields on Inherit is a reset, not an empty override', async () => {
    const { c, api, s } = setup({
      gameLaunch: vi.fn(async () => game({ override: { wrappers: ['gamemode'] }, effective: { ...none, wrappers: ['gamemode'] } })),
    });
    await c.load();
    const d = s().draft!;
    expect(d.inherit.wrappers).toBe(false);
    c.edit({ ...d, inherit: { ...d.inherit, wrappers: true } }, true);
    await c.flush();
    expect(api.putGameLaunch).not.toHaveBeenCalled();
    expect(api.resetGameLaunch).toHaveBeenCalledTimes(1);
  });

  it('after a save an inherited field shows the default again', async () => {
    const { c, s } = setup();
    await c.load();
    const d = s().draft!;
    c.edit({ form: { ...d.form, extra: ['-x'] }, inherit: { ...d.inherit, wrappers: false, extra: true } }, true);
    await c.flush();
    expect(s().draft?.form.extra).toEqual([]);
  });

  it('Reset deletes the override and says so', async () => {
    const { c, s, api } = setup();
    await c.load();
    await c.reset();
    expect(api.resetGameLaunch).toHaveBeenCalledWith('steam:620');
    expect(s().note).toEqual({ text: BACK_TO_DEFAULTS, tone: 'info' });
    expect(s().server?.envAllowed).toEqual(['DXVK_HUD', 'MANGOHUD']);
  });

  it('a 409 keeps the allowed names; "too many games" has no state to show and is reported', async () => {
    const cur = game({ rev: 'g9', override: { extra_args: ['-y'] }, effective: { ...none, extra_args: ['-y'] } });
    const a = setup({ putGameLaunch: vi.fn().mockRejectedValue(new ApiError('changed elsewhere', 409, { current: cur })) });
    await a.c.load();
    const d = a.s().draft!;
    a.c.edit({ form: d.form, inherit: { ...d.inherit, env: false } }, true);
    await a.c.flush();
    expect(a.s().server?.rev).toBe('g9');
    expect(a.s().server?.envAllowed).toEqual(['DXVK_HUD', 'MANGOHUD']);
    expect(a.s().draft?.form.extra).toEqual(['-y']);

    const b = setup({
      putGameLaunch: vi.fn().mockRejectedValue(new ApiError('too many games have launch overrides', 409, { error: 'x' })),
    });
    await b.c.load();
    b.c.edit({ form: b.s().draft!.form, inherit: { ...b.s().draft!.inherit, env: false } }, true);
    await b.c.flush();
    expect(b.s().note).toEqual({ text: 'too many games have launch overrides', tone: 'error' });
  });
});
