import { describe, expect, it } from 'vitest';
import type { LaunchOpts } from '../../state/api';
import {
  allInherit,
  emptyForm,
  formOf,
  gsArgs,
  gsParse,
  gsProblems,
  hasLaunchOptions,
  inheritOf,
  launchIdOf,
  optionsOf,
  overrideOf,
  summary,
  trimTail,
  withAllow,
} from './form';

const full: LaunchOpts = {
  wrappers: ['mangohud', 'gamemode', { gamescope: { args: ['-W', '1920', '-H', '1080', '-r', '60', '-f', '--fsr'] } }],
  env: { DXVK_HUD: 'fps' },
  custom: ['/usr/bin/mytool', '--x'],
  extra_args: ['-windowed'],
};

describe('form <-> JSON', () => {
  it('round-trips, with the wrappers in a fixed order', () => {
    const o = optionsOf(formOf(full));
    expect(o.wrappers).toEqual(['mangohud', 'gamemode', full.wrappers[2]]);
    expect({ ...o, wrappers: full.wrappers }).toEqual(full);
  });
  it('an empty form is an empty option set', () => {
    expect(optionsOf(emptyForm())).toEqual({ wrappers: [], env: {}, custom: null, extra_args: [] });
  });
  it('blank rows at the end are dropped, blank ones in the middle stay', () => {
    expect(trimTail(['a', '', 'b', '', ''])).toEqual(['a', '', 'b']);
    const f = { ...emptyForm(), custom: ['tool', ''], extra: ['', ''] };
    expect(optionsOf(f)).toMatchObject({ custom: ['tool'], extra_args: [] });
    expect(optionsOf({ ...emptyForm(), custom: [''] }).custom).toBeNull();
  });
  it('env rows without a name are not sent', () => {
    expect(
      optionsOf({
        ...emptyForm(),
        env: [
          ['', 'x'],
          ['MANGOHUD', '1'],
        ],
      }).env,
    ).toEqual({ MANGOHUD: '1' });
  });
});

describe('gamescope flags', () => {
  it('maps fields to the validated flags and back', () => {
    const g = gsParse(['-W', '1920', '-w', '1280', '-r', '60', '-b', '--fsr']);
    expect(g.num).toEqual({ '-W': '1920', '-w': '1280', '-r': '60' });
    expect(g.flags).toEqual({ '-b': true, '--fsr': true });
    expect(gsArgs(g)).toEqual(['-W', '1920', '-w', '1280', '-r', '60', '-b', '--fsr']);
  });
  it('blank numbers are left out and unticked flags too', () => {
    expect(gsArgs({ on: true, num: { '-W': ' ', '-H': '720' }, flags: { '-f': false } })).toEqual(['-H', '720']);
  });
  it('numbers outside what hostd takes are reported in words', () => {
    const bad = (k: string, v: string) => gsProblems({ on: true, num: { [k]: v }, flags: {} });
    expect(bad('-W', '1920')).toEqual([]);
    expect(bad('-r', '1000')).toEqual([]);
    for (const [k, v] of [
      ['-r', '1001'],
      ['-W', '0'],
      ['-H', '-5'],
      ['-w', 'abc'],
      ['-h', '12.5'],
      ['-W', '16385'],
      ['-W', '123456'],
    ])
      expect(bad(k, v), `${k} ${v}`).toHaveLength(1);
    expect(bad('-r', '0')[0]).toBe('Refresh rate (Hz) needs a whole number from 1 to 1000.');
    expect(gsProblems({ on: false, num: { '-W': 'x' }, flags: {} })).toEqual([]);
  });
});

describe('allow custom commands', () => {
  it('turning it off clears the command, turning it on keeps the form', () => {
    const f = formOf(full);
    expect(withAllow(f, false).custom).toEqual([]);
    expect(optionsOf(withAllow(f, false)).custom).toBeNull();
    expect(withAllow(f, true)).toBe(f);
  });
});

describe('override', () => {
  it('sends only the fields marked Override; an empty command is [] (clears the default)', () => {
    const f = { ...formOf(full), custom: [] };
    const o = overrideOf(f, { wrappers: true, env: false, custom: false, extra: true });
    expect(o).toEqual({ env: { DXVK_HUD: 'fps' }, custom: [] });
  });
  it('reads which fields an override has', () => {
    expect(inheritOf(null)).toEqual({ wrappers: true, env: true, custom: true, extra: true });
    const i = inheritOf({ wrappers: [], env: null, custom: [], extra_args: undefined });
    expect(i).toEqual({ wrappers: false, env: true, custom: false, extra: true });
    expect(allInherit(i)).toBe(false);
    expect(allInherit(inheritOf({}))).toBe(true);
  });
});

describe('summary', () => {
  it('says what the effective options do', () => {
    expect(summary(full)).toBe(
      'Runs under gamescope (screen 1920 by 1080, 60 Hz, fullscreen, FSR), GameMode and MangoHud. Your command wraps the game: /usr/bin/mytool --x. Environment: DXVK_HUD=fps. Extra arguments: -windowed.',
    );
  });
  it('is short for little and plain for nothing', () => {
    expect(summary({ ...full, wrappers: ['mangohud'], env: {}, custom: null, extra_args: [] })).toBe('Runs under MangoHud.');
    expect(summary({ wrappers: [{ gamescope: { args: [] } }, 'gamemode'], env: {}, custom: null, extra_args: [] })).toBe(
      'Runs under gamescope and GameMode.',
    );
    expect(summary({ wrappers: [], env: {}, custom: null, extra_args: [] })).toBe('Starts the game as it is, with no options.');
  });
});

describe('game ids and caps', () => {
  it('uses the Stage 1 id, else the old number', () => {
    expect(launchIdOf({ id: 'emu:dolphin:mario-kart-wii', appid: 3907440760 })).toBe('emu:dolphin:mario-kart-wii');
    expect(launchIdOf({ appid: 620 })).toBe('620');
  });
  it('the section needs the cap', () => {
    expect(hasLaunchOptions({ caps: { launchOptions: true } })).toBe(true);
    expect(hasLaunchOptions({ caps: { launchOptions: false } })).toBe(false);
    expect(hasLaunchOptions({})).toBe(false);
  });
});
