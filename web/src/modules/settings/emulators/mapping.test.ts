import { describe, expect, it } from 'vitest';
import classicGet from '../../../../../crates/brisk-hostd/tests/fixtures/controls/v2-classic-get.json';
import classicPut from '../../../../../crates/brisk-hostd/tests/fixtures/controls/v2-classic-put.json';
import gcGet from '../../../../../crates/brisk-hostd/tests/fixtures/controls/v2-gamecube-get.json';
import gcPut from '../../../../../crates/brisk-hostd/tests/fixtures/controls/v2-gamecube-put.json';
import type { ControllerType } from '../../../state/api';
import { PAD_LABEL, PAD_NAMES } from '../pad.svg';
import { accepts, bind, CONTROL_SETS, controlOf, DEFAULTS, drawnLabels, idsOf, kindOf, PAD_BY_INDEX, padHolder, unbound } from './mapping';

const TYPES: ControllerType[] = ['classic', 'gamecube'];
const fixtures = { classic: [classicGet, classicPut], gamecube: [gcGet, gcPut] };

describe('contract with the fixtures', () => {
  for (const t of TYPES) {
    it(`${t}: key sets equal the control set and every value is accepted`, () => {
      for (const f of fixtures[t]) {
        expect(Object.keys(f.bindings).sort()).toEqual(idsOf(t).sort());
        for (const [id, pad] of Object.entries(f.bindings)) expect(accepts(t, id, pad as string), `${id}=${pad}`).toBe(true);
      }
      const get = t === 'classic' ? classicGet : gcGet;
      expect(Object.keys(get.custom).sort()).toEqual(idsOf(t).sort());
      expect(Object.keys(get.deadZone).sort()).toEqual(
        idsOf(t)
          .filter((i) => controlOf(t, i).kind === 'stick')
          .sort(),
      );
    });
    it(`${t}: the defaults are valid, one to one, and equal the fixture`, () => {
      expect(DEFAULTS[t]).toEqual(fixtures[t][0].bindings);
      const v = Object.values(DEFAULTS[t]);
      expect(new Set(v).size).toBe(v.length);
    });
  }
  it('sizes: 14 classic controls, 11 GameCube controls', () => {
    expect(CONTROL_SETS.classic).toHaveLength(14);
    expect(CONTROL_SETS.gamecube).toHaveLength(11);
  });
});

describe('pad names', () => {
  it('Gamepad API indexes map to the drawing names (Back, Start, Home print for View, Menu, Guide)', () => {
    expect(PAD_BY_INDEX[8]).toBe('View');
    expect(PAD_BY_INDEX[9]).toBe('Menu');
    expect(PAD_BY_INDEX[16]).toBe('Guide');
    expect([8, 9, 16].map((i) => PAD_LABEL[PAD_BY_INDEX[i]])).toEqual(['Back', 'Start', 'Home']);
    expect(PAD_BY_INDEX[10]).toBeUndefined();
    expect(PAD_BY_INDEX[11]).toBeUndefined();
    for (const i of [12, 13, 14, 15]) expect(PAD_BY_INDEX[i]).toBe('D-pad');
    for (const n of Object.values(PAD_BY_INDEX)) expect(PAD_NAMES).toContain(n);
  });
  it('kindOf', () => {
    expect(['A', 'Left stick', 'Right stick', 'D-pad', 'Guide'].map(kindOf)).toEqual(['button', 'stick', 'stick', 'dpad', 'button']);
  });
});

describe('bind', () => {
  const c = DEFAULTS.classic;
  it('a plain bind frees the old name', () => {
    const g = { ...DEFAULTS.gamecube };
    const r = bind('gamecube', g, 'z', 'View'); // View is free in GameCube
    expect(r).toMatchObject({ swappedWith: null, refused: null });
    expect(r.bindings.z).toBe('View');
    expect(padHolder(r.bindings, 'RB')).toBeNull();
    expect(unbound(r.bindings)).toContain('RB');
  });
  it('binding a held name swaps', () => {
    const r = bind('classic', c, 'a', 'LB'); // zl holds LB
    expect(r.swappedWith).toBe('zl');
    expect(r.bindings.a).toBe('LB');
    expect(r.bindings.zl).toBe('B');
    expect(new Set(Object.values(r.bindings)).size).toBe(14);
  });
  it('the same name is a no-op', () => {
    const r = bind('classic', c, 'a', 'B');
    expect(r.bindings).toBe(c);
  });
  it('kind match: a stick takes only sticks, a button never a stick, the d-pad only the d-pad', () => {
    expect(bind('classic', c, 'ls', 'A').bindings).toBe(c);
    expect(bind('classic', c, 'a', 'Left stick').bindings).toBe(c);
    expect(bind('classic', c, 'dp', 'A').bindings).toBe(c);
    expect(bind('classic', c, 'a', 'D-pad').bindings).toBe(c);
    expect(bind('classic', c, 'ls', 'Right stick').swappedWith).toBe('rs');
  });
  it('a custom control cannot take a held name: refused, nothing changes', () => {
    const cu = { ...c, zl: null };
    const r = bind('classic', cu, 'zl', 'A');
    expect(r.bindings).toBe(cu);
    expect(r.refused).toBe('A is used by B. Change B first.');
    expect(bind('classic', { ...c, zl: null }, 'zl', 'LB').refused).toBeNull(); // LB is free now: fine
    expect(bind('classic', cu, 'zl', 'LB').bindings.zl).toBe('LB');
  });
  it('the refusal uses the drawing label', () => {
    const r = bind('classic', { ...c, a: null }, 'a', 'View');
    expect(r.refused).toBe('Back is used by Minus. Change Minus first.');
  });
  it('GameCube leaves three pad names unbound by default; Classic none', () => {
    expect(unbound(DEFAULTS.gamecube).sort()).toEqual(['Guide', 'LB', 'View']);
    expect(unbound(DEFAULTS.classic)).toEqual([]);
  });
  it('drawnLabels: pad name -> what it does, with symbols for Classic minus and plus', () => {
    expect(drawnLabels('classic', c)).toMatchObject({ B: 'A', LB: 'ZL', View: '−', Menu: '+', 'Left stick': 'Left stick' });
    expect(drawnLabels('gamecube', DEFAULTS.gamecube).View).toBeUndefined();
  });
});
