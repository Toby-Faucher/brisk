import { describe, expect, it } from 'vitest';
import { ACCEPTS, BUTTON_NAMES, bind, CONTROL_IDS, CONTROLS, PAD_BY_INDEX, PAD_NAMES, XBOX_DEFAULT } from './classic';
import get from '../../../../../crates/brisk-hostd/tests/fixtures/controls/get.json';
import put from '../../../../../crates/brisk-hostd/tests/fixtures/controls/put.json';

describe('classic', () => {
  it('lists the 14 controls and the doc allowlist', () => {
    expect(CONTROL_IDS).toEqual(['a', 'b', 'x', 'y', 'l', 'r', 'zl', 'zr', 'minus', 'plus', 'home', 'ls', 'rs', 'dp']);
    for (const id of CONTROL_IDS.slice(0, 11)) expect(ACCEPTS[id]).toEqual(BUTTON_NAMES);
    expect(BUTTON_NAMES).toHaveLength(11);
    expect(ACCEPTS.ls).toEqual(['Left stick', 'Right stick']);
    expect(ACCEPTS.rs).toEqual(['Left stick', 'Right stick']);
    expect(ACCEPTS.dp).toEqual(['D-pad']);
    expect(CONTROLS.map((c) => c.kind).filter((k) => k === 'button')).toHaveLength(11);
  });

  it('XBOX_DEFAULT is a full one-to-one pairing inside the allowlist', () => {
    expect(Object.keys(XBOX_DEFAULT)).toEqual(CONTROL_IDS);
    for (const id of CONTROL_IDS) expect(ACCEPTS[id]).toContain(XBOX_DEFAULT[id]);
    expect(new Set(Object.values(XBOX_DEFAULT)).size).toBe(14);
    expect(XBOX_DEFAULT).toMatchObject({ a: 'B', b: 'A', x: 'Y', y: 'X', l: 'LT', r: 'RT', zl: 'LB', zr: 'RB' });
  });

  it('maps pad indexes, with stick clicks mapping to nothing', () => {
    expect(PAD_BY_INDEX[0]).toBe('A');
    expect(PAD_BY_INDEX[16]).toBe('Guide');
    expect(PAD_BY_INDEX[10]).toBeUndefined();
    expect(PAD_BY_INDEX[11]).toBeUndefined();
    for (const i of [12, 13, 14, 15]) expect(PAD_BY_INDEX[i]).toBe('D-pad');
    for (const n of Object.values(PAD_BY_INDEX)) expect(PAD_NAMES).toContain(n);
  });

  it('swaps with the control that holds the name (the pairing is always full)', () => {
    const r = bind(XBOX_DEFAULT, 'a', 'LB');
    expect(r.swappedWith).toBe('zl');
    expect(r.bindings.a).toBe('LB');
    expect(r.bindings.zl).toBe('B'); // zl takes a's old binding
    expect(new Set(Object.values(r.bindings)).size).toBe(14);
    expect(XBOX_DEFAULT.a).toBe('B'); // input untouched
  });

  it('swaps the two sticks', () => {
    const r = bind(XBOX_DEFAULT, 'ls', 'Right stick');
    expect(r.swappedWith).toBe('rs');
    expect(r.bindings).toMatchObject({ ls: 'Right stick', rs: 'Left stick' });
  });

  it('binding the same name, or one the control does not take, changes nothing', () => {
    expect(bind(XBOX_DEFAULT, 'a', 'B')).toEqual({ bindings: XBOX_DEFAULT, swappedWith: null });
    expect(bind(XBOX_DEFAULT, 'a', 'Left stick').bindings).toBe(XBOX_DEFAULT);
    expect(bind(XBOX_DEFAULT, 'ls', 'A').bindings).toBe(XBOX_DEFAULT);
    expect(bind(XBOX_DEFAULT, 'dp', 'Left stick').bindings).toBe(XBOX_DEFAULT);
  });

  it('a custom control (null) takes a held name and hands the other a free one', () => {
    const custom = { ...XBOX_DEFAULT, a: null, rs: null };
    const r = bind(custom, 'a', 'LB');
    expect(r.swappedWith).toBe('zl');
    expect(r.bindings.a).toBe('LB');
    expect(r.bindings.zl).toBe('B'); // the first button name nobody holds
    const s = bind(custom, 'rs', 'Left stick');
    expect(s.bindings).toMatchObject({ rs: 'Left stick', ls: 'Right stick' });
    expect(bind(custom, 'a', 'B').swappedWith).toBeNull(); // B is free: plain bind
    expect(bind(custom, 'a', 'B').bindings.a).toBe('B');
  });
});

describe('shared fixtures', () => {
  it('get.json and put.json use the control ids and allowed names', () => {
    for (const b of [get.bindings, put.bindings]) {
      expect(Object.keys(b)).toEqual(CONTROL_IDS);
      for (const id of CONTROL_IDS) expect(ACCEPTS[id]).toContain((b as Record<string, string>)[id]);
    }
    expect(Object.keys(get.custom)).toEqual(CONTROL_IDS);
    expect(get.bindings).toEqual(XBOX_DEFAULT);
    expect(Object.keys(put.deadZone)).toEqual(['ls', 'rs']);
  });
});
