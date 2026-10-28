import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, ApiError, type GameLaunch, type LaunchDefaults, type LaunchOpts } from '../../state/api';
import Settings from '../settings/Settings.svelte';
import DefaultsEditor from './DefaultsEditor.svelte';
import GameLaunch_ from './GameLaunch.svelte';
import { CONFLICT, SAVED } from './store';

const none: LaunchOpts = { wrappers: [], env: {}, custom: null, extra_args: [] };
const ENV = ['DXVK_HUD', 'MANGOHUD', 'SDL_VIDEODRIVER'];
const defs = (o: Partial<LaunchDefaults> = {}): LaunchDefaults => ({
  rev: 'r1',
  allowCustomCommands: false,
  options: none,
  envAllowed: ENV,
  ...o,
});
const LINE = '/opt/brisk/brisk-hostd wrap steam:620 -- %command%';
const gl = (o: Partial<GameLaunch> = {}): GameLaunch => ({
  id: 'steam:620',
  rev: 'g1',
  override: null,
  effective: none,
  problems: [],
  allowCustomCommands: false,
  steamLine: LINE,
  ...o,
});
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const lastPut = () => vi.mocked(api.putLaunchDefaults).mock.calls.at(-1)![0];
const copied = () => document.querySelector('#lo-copy')?.textContent?.trim() ?? '';
const alerts = () => [...document.querySelectorAll('[role="alert"]')].map((e) => e.textContent).join(' ');
const status = () => [...document.querySelectorAll('[role="status"]')].at(-1)?.textContent?.trim() ?? '';

let cur: LaunchDefaults;
beforeEach(() => {
  cur = defs();
  vi.spyOn(api, 'launchDefaults').mockImplementation(async () => structuredClone(cur));
  vi.spyOn(api, 'putLaunchDefaults').mockImplementation(
    async (b) => (cur = defs({ rev: `r${Math.random()}`, allowCustomCommands: b.allowCustomCommands, options: b.options })),
  );
});
afterEach(() => vi.restoreAllMocks());

const defaults = async () => {
  await render(DefaultsEditor, { uid: 't' });
  await expect.element(page.getByRole('checkbox', { name: /^MangoHud/ })).toBeInTheDocument();
};
const box = (name: RegExp | string) => page.getByRole('checkbox', { name });

describe('defaults editor', () => {
  it('shows a loading state, then the form', async () => {
    let ok!: (v: LaunchDefaults) => void;
    vi.mocked(api.launchDefaults).mockReturnValueOnce(new Promise((r) => (ok = r)));
    await render(DefaultsEditor, { uid: 't' });
    await expect.element(page.getByText('Loading the launch options...')).toBeInTheDocument();
    expect(document.querySelector('.ld')?.getAttribute('aria-busy')).toBe('true');
    ok(defs());
    await expect.element(box(/^MangoHud/)).toBeInTheDocument();
    expect(document.querySelector('.ld')?.getAttribute('aria-busy')).toBe('false');
  });

  it('a failed load says so and Try again loads', async () => {
    vi.mocked(api.launchDefaults).mockRejectedValueOnce(new ApiError('x', 500));
    await render(DefaultsEditor, { uid: 't' });
    await expect.poll(alerts).toContain('Could not load the launch options.');
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.element(box(/^MangoHud/)).toBeInTheDocument();
  });

  it('a wrapper box saves at once and the status says when it is used', async () => {
    await defaults();
    await userEvent.click(box(/^MangoHud/));
    await expect.poll(status).toBe(SAVED);
    expect(lastPut()).toEqual({ rev: 'r1', allowCustomCommands: false, options: { ...none, wrappers: ['mangohud'] } });
  });

  it('keyboard: Space toggles a box, the rest of the form is reachable by Tab', async () => {
    await defaults();
    box(/^GameMode/)
      .element()
      .focus();
    await userEvent.keyboard(' ');
    await expect.poll(() => lastPut().options.wrappers).toEqual(['gamemode']);
    await userEvent.tab();
    expect(document.activeElement).toBe(box(/^gamescope/).element());
  });

  it('gamescope: fields map to flags, a bad number is told in words and nothing is sent', async () => {
    await defaults();
    await userEvent.click(box(/^gamescope/));
    await expect.element(page.getByRole('group', { name: 'gamescope settings' })).toBeInTheDocument();
    await userEvent.click(page.getByLabelText('Refresh rate (Hz)'));
    await userEvent.keyboard('5000');
    await expect.element(page.getByRole('alert')).toHaveTextContent('Refresh rate (Hz) needs a whole number from 1 to 1000.');
    await expect.element(page.getByLabelText('Refresh rate (Hz)')).toHaveAttribute('aria-invalid', 'true');
    await wait(800);
    expect(vi.mocked(api.putLaunchDefaults).mock.calls.filter((c) => JSON.stringify(c).includes('5000'))).toHaveLength(0);
    await userEvent.clear(page.getByLabelText('Refresh rate (Hz)'));
    await userEvent.keyboard('60');
    await userEvent.click(box('Fullscreen'));
    await userEvent.click(box('FSR upscaling'));
    await expect.poll(() => lastPut().options.wrappers).toEqual([{ gamescope: { args: ['-r', '60', '-f', '--fsr'] } }]);
  });

  it('environment: pick a name from the list, give a value, remove it', async () => {
    await defaults();
    await userEvent.click(page.getByRole('button', { name: 'Add a variable' }));
    const pick = page.getByRole('combobox', { name: 'Variable 1' });
    await expect.element(pick).toHaveValue('DXVK_HUD');
    expect([...pick.element().querySelectorAll('option')].map((o) => o.value)).toEqual(ENV);
    await userEvent.click(page.getByLabelText('Value of DXVK_HUD'));
    await userEvent.keyboard('fps');
    await expect.poll(() => lastPut().options.env, { timeout: 3000 }).toEqual({ DXVK_HUD: 'fps' });
    await userEvent.click(page.getByRole('button', { name: 'Remove DXVK_HUD' }));
    await expect.poll(() => lastPut().options.env).toEqual({});
  });

  it('custom command: hidden and explained while the switch is off; on shows an argument list, off clears it in the same save', async () => {
    await defaults();
    const sw = page.getByRole('switch', { name: 'Allow custom commands' });
    await expect.element(sw).not.toBeChecked();
    await expect.element(page.getByText('A command runs on this machine as you, so it stays off until you allow it.')).toBeInTheDocument();
    expect(page.getByRole('textbox', { name: 'Program' }).elements()).toHaveLength(0);
    expect(page.getByRole('button', { name: 'Add the program' }).elements()).toHaveLength(0);

    await userEvent.click(sw);
    await expect.poll(() => lastPut().allowCustomCommands).toBe(true);
    await userEvent.click(page.getByRole('button', { name: 'Add the program' }));
    await expect.element(page.getByRole('textbox', { name: 'Program' })).toHaveFocus();
    await userEvent.keyboard('/usr/bin/mytool');
    await userEvent.click(page.getByRole('button', { name: 'Add custom argument' }));
    await userEvent.keyboard('--fast');
    await expect.poll(() => lastPut().options.custom, { timeout: 3000 }).toEqual(['/usr/bin/mytool', '--fast']);
    await expect.element(page.getByText(/Nothing goes through a shell/)).toBeInTheDocument();

    await userEvent.click(sw);
    await expect.poll(() => lastPut().allowCustomCommands).toBe(false);
    expect(lastPut().options.custom).toBeNull();
    expect(page.getByRole('textbox', { name: 'Program' }).elements()).toHaveLength(0);
  });

  it('extra arguments: one per row', async () => {
    await defaults();
    await userEvent.click(page.getByRole('button', { name: 'Add extra argument' }));
    await userEvent.keyboard('-windowed');
    await expect.poll(() => lastPut().options.extra_args, { timeout: 3000 }).toEqual(['-windowed']);
  });

  it('hostd refuses: the reasons are listed in an alert and the form keeps the edit', async () => {
    vi.mocked(api.putLaunchDefaults).mockRejectedValueOnce(
      new ApiError('these launch options are not allowed', 400, { problems: ['gamescope: flag not allowed: -x'] }),
    );
    await defaults();
    await userEvent.click(box(/^MangoHud/));
    await expect.poll(alerts).toContain('gamescope: flag not allowed: -x');
    await expect.element(box(/^MangoHud/)).toBeChecked();
  });

  it('another device changed it: the latest is shown with a note', async () => {
    vi.mocked(api.putLaunchDefaults).mockRejectedValueOnce(
      new ApiError('changed elsewhere', 409, { current: defs({ rev: 'r9', options: { ...none, wrappers: ['gamemode'] } }) }),
    );
    await defaults();
    await userEvent.click(box(/^MangoHud/));
    await expect.poll(status).toBe(CONFLICT);
    await expect.element(box(/^GameMode/)).toBeChecked();
    await expect.element(box(/^MangoHud/)).not.toBeChecked();
  });

  it('a failed save has a Try again', async () => {
    vi.mocked(api.putLaunchDefaults).mockRejectedValueOnce(new ApiError('boom', 500));
    await defaults();
    await userEvent.click(box(/^MangoHud/));
    await expect.poll(alerts).toContain('Could not save the launch options.');
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.poll(status).toBe(SAVED);
  });

  it('every control has a name (a11y)', async () => {
    cur = defs({
      allowCustomCommands: true,
      options: { wrappers: [{ gamescope: { args: ['-W', '1280'] } }], env: { MANGOHUD: '1' }, custom: ['tool'], extra_args: ['-x'] },
    });
    await defaults();
    for (const el of document.querySelectorAll('input, select, button')) {
      const named = (el as HTMLElement).getAttribute('aria-label') || (el as HTMLInputElement).labels?.length || el.textContent?.trim();
      expect(named, el.outerHTML).toBeTruthy();
    }
    expect(page.getByRole('region').elements().length + page.getByRole('heading').elements().length).toBeGreaterThan(3);
  });
});

describe('a game panel', () => {
  let g: GameLaunch;
  const open = async (o: Partial<GameLaunch> = {}, id = 'steam:620') => {
    g = gl(o);
    vi.spyOn(api, 'gameLaunch').mockImplementation(async () => structuredClone(g));
    vi.spyOn(api, 'putGameLaunch').mockImplementation(
      async (_i, b) =>
        (g = gl({
          ...g,
          rev: `g${Math.random()}`,
          override: b.override,
          effective: { ...g.effective, ...(b.override.wrappers ? { wrappers: b.override.wrappers } : {}) },
        })),
    );
    vi.spyOn(api, 'resetGameLaunch').mockImplementation(async () => (g = gl({ ...g, override: null, effective: cur.options })));
    await render(GameLaunch_, { game: { id, appid: 620 } });
    await expect.element(page.getByRole('heading', { name: 'Launch options' })).toBeInTheDocument();
    await expect.element(page.getByText(/follows the defaults|has its own/)).toBeInTheDocument();
  };
  const editor = () => userEvent.click(page.getByText('Change launch options'));
  const mode = (field: string, v: 'Inherit' | 'Override') =>
    page.getByRole('radiogroup', { name: new RegExp(`^${field}: follow`) }).getByRole('radio', { name: v });

  it('loading, then the effective result in words', async () => {
    let ok!: (v: GameLaunch) => void;
    vi.spyOn(api, 'gameLaunch').mockReturnValue(new Promise((r) => (ok = r)));
    await render(GameLaunch_, { game: { id: 'steam:620', appid: 620 } });
    await expect.element(page.getByText('Loading the launch options...')).toBeInTheDocument();
    ok(gl({ effective: { ...none, wrappers: ['mangohud', 'gamemode'], env: { DXVK_HUD: 'fps' } } }));
    await expect.element(page.getByText('Runs under GameMode and MangoHud. Environment: DXVK_HUD=fps.')).toBeInTheDocument();
    expect(api.gameLaunch).toHaveBeenCalledWith('steam:620');
  });

  it('an old machine is asked with the bare number', async () => {
    vi.spyOn(api, 'gameLaunch').mockResolvedValue(gl());
    await render(GameLaunch_, { game: { appid: 620 } });
    await expect.poll(() => vi.mocked(api.gameLaunch).mock.calls[0]?.[0]).toBe('620');
  });

  it('a failed load can be retried', async () => {
    vi.spyOn(api, 'gameLaunch').mockRejectedValueOnce(new ApiError('x', 500)).mockResolvedValue(gl());
    await render(GameLaunch_, { game: { id: 'steam:620', appid: 620 } });
    await expect.poll(alerts).toContain('Could not load the launch options.');
    await userEvent.click(page.getByRole('button', { name: 'Try again' }));
    await expect.element(page.getByText('Starts the game as it is, with no options.')).toBeInTheDocument();
  });

  it('Steam: the line, a hint, and Copy', async () => {
    await open();
    const line = page.getByRole('textbox', { name: 'Steam launch option' });
    await expect.element(line).toHaveValue(LINE);
    await expect.element(line).toHaveAttribute('readonly');
    await expect.element(page.getByText(/Paste this once into the game's Launch Options in Steam/)).toBeInTheDocument();
    const w = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    await userEvent.click(page.getByRole('button', { name: 'Copy' }));
    expect(w).toHaveBeenCalledWith(LINE);
    await expect.poll(copied).toBe('Copied.');
  });

  it('Copy falls back to selecting the line when the clipboard is refused', async () => {
    await open();
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(new DOMException('no', 'NotAllowedError'));
    vi.spyOn(document, 'execCommand').mockReturnValue(false);
    await userEvent.click(page.getByRole('button', { name: 'Copy' }));
    await expect.poll(copied).toBe('Selected. Press Ctrl+C to copy.');
    const f = page.getByRole('textbox', { name: 'Steam launch option' }).element() as HTMLInputElement;
    expect(document.activeElement).toBe(f);
    expect([f.selectionStart, f.selectionEnd]).toEqual([0, LINE.length]);
  });

  it('an emulator game has no Steam line and no Copy', async () => {
    await open({ id: 'emu:dolphin:mkw', steamLine: null }, 'emu:dolphin:mkw');
    expect(page.getByRole('button', { name: 'Copy' }).elements()).toHaveLength(0);
    expect(page.getByRole('textbox', { name: 'Steam launch option' }).elements()).toHaveLength(0);
    expect(api.gameLaunch).toHaveBeenCalledWith('emu:dolphin:mkw');
  });

  it('inherited fields are shown and cannot be edited until set to Override', async () => {
    cur = defs({ options: { ...none, wrappers: ['mangohud'] } });
    await open({ effective: cur.options });
    await editor();
    await expect.element(mode('Wrappers', 'Inherit')).toBeChecked();
    await expect.element(box(/^MangoHud/)).toBeChecked();
    await expect.element(box(/^MangoHud/)).toBeDisabled();
    await userEvent.click(mode('Wrappers', 'Override'));
    await expect.element(box(/^MangoHud/)).toBeEnabled();
    await userEvent.click(box(/^GameMode/));
    await expect
      .poll(() => vi.mocked(api.putGameLaunch).mock.calls.at(-1)?.[1])
      .toMatchObject({ override: { wrappers: ['mangohud', 'gamemode'] } });
    await expect.poll(status).toBe(SAVED);
    await expect.element(page.getByText(/has its own wrappers/)).toBeInTheDocument();
  });

  it('keyboard: radios move with the arrow keys', async () => {
    await open();
    await editor();
    mode('Environment variables', 'Inherit').element().focus();
    await userEvent.keyboard('{ArrowRight}');
    await expect.element(mode('Environment variables', 'Override')).toBeChecked();
    await expect.poll(() => vi.mocked(api.putGameLaunch).mock.calls.at(-1)?.[1].override).toEqual({ env: {} });
  });

  it('Reset is disabled when there is no override', async () => {
    await open();
    await expect.element(page.getByRole('button', { name: 'Reset to the defaults' })).toBeDisabled();
  });

  it('Reset deletes the override', async () => {
    await open({ override: { extra_args: ['-x'] }, effective: { ...none, extra_args: ['-x'] } });
    await userEvent.click(page.getByRole('button', { name: 'Reset to the defaults' }));
    expect(api.resetGameLaunch).toHaveBeenCalledWith('steam:620');
    await expect.poll(status).toBe('Back to the defaults.');
    await expect.element(page.getByText(/This game follows the defaults\./)).toBeInTheDocument();
  });

  it('options hostd would ignore are listed as problems', async () => {
    await open({ problems: ['custom commands are switched off; turn on Allow custom commands in Settings'] });
    await expect.poll(alerts).toContain('the game starts without them');
    await expect.poll(alerts).toContain('turn on Allow custom commands in Settings');
  });

  it('custom commands off: the field is not offered and a link says where to turn it on', async () => {
    await open({ allowCustomCommands: false });
    await editor();
    expect(page.getByRole('textbox', { name: 'Program' }).elements()).toHaveLength(0);
    await expect.element(page.getByText('A command runs on this machine as you, so it stays off until you allow it.')).toBeInTheDocument();
    await expect.element(page.getByRole('link', { name: 'Allow custom commands in Settings' })).toHaveAttribute('href', '/settings#launch');
  });

  it('custom commands on: Inherit shows the default command disabled, Override unlocks it', async () => {
    cur = defs({ allowCustomCommands: true, options: { ...none, custom: ['mytool', '-a'] } });
    await open({ allowCustomCommands: true, effective: cur.options });
    await editor();
    const prog = page.getByRole('textbox', { name: 'Program' });
    await expect.element(prog).toHaveValue('mytool');
    await expect.element(prog).toBeDisabled();
    await userEvent.click(mode('Your own command', 'Override'));
    await expect.element(prog).toBeEnabled();
  });

  it('a 409 shows what the other device saved', async () => {
    await open();
    vi.mocked(api.putGameLaunch).mockRejectedValueOnce(
      new ApiError('changed elsewhere', 409, {
        current: gl({ rev: 'g9', override: { extra_args: ['-z'] }, effective: { ...none, extra_args: ['-z'] } }),
      }),
    );
    await editor();
    await userEvent.click(mode('Wrappers', 'Override'));
    await expect.poll(status).toBe(CONFLICT);
    await expect.element(page.getByText(/Extra arguments: -z\./)).toBeInTheDocument();
  });

  it('hostd refuses: the reasons show', async () => {
    await open();
    vi.mocked(api.putGameLaunch).mockRejectedValueOnce(new ApiError('no', 400, { problems: ['gamescope: flag not allowed: -x'] }));
    await editor();
    await userEvent.click(mode('Wrappers', 'Override'));
    await expect.element(page.getByText('These options were not saved:')).toBeInTheDocument();
    await expect.element(page.getByText('gamescope: flag not allowed: -x')).toBeInTheDocument();
  });
});

describe('Settings', () => {
  it('the Launch tab holds the defaults editor', async () => {
    history.replaceState(null, '', '#launch');
    await render(Settings);
    await expect.element(page.getByRole('tab', { name: 'Launch' })).toHaveAttribute('aria-selected', 'true');
    await expect.element(page.getByRole('heading', { name: 'Launch options for all games' })).toBeInTheDocument();
    await expect.element(box(/^MangoHud/)).toBeInTheDocument();
  });

  it('Emulators > Dolphin > Launch options is the same editor, with a note that it is shared', async () => {
    history.replaceState(null, '', '#emulators/dolphin/launch');
    await render(Settings);
    await expect.element(page.getByRole('tab', { name: 'Launch options' })).toHaveAttribute('aria-selected', 'true');
    await expect.element(page.getByText(/shared by every game on this machine, emulator games included/)).toBeInTheDocument();
    await expect.element(page.getByRole('link', { name: 'open' })).toHaveAttribute('href', '#launch');
    await expect.element(box(/^MangoHud/)).toBeInTheDocument();
    await userEvent.click(box(/^MangoHud/));
    await expect.poll(() => lastPut().options.wrappers).toEqual(['mangohud']);
  });
});
