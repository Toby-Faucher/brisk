import { describe, expect, it } from 'vitest';
import { BUTTON_FLAGS, type PadLike } from '../../lib/gamepad';
import {
  DEFAULT_DEADZONE, DriftCheck, PollRate, RangeTest, Trail, driftVerdict, mappingRows, padChoices, padView, stickView, suggestDeadzone,
} from './controllertest';

function pad(over: Partial<PadLike> = {}, pressed: Record<number, number> = {}, axes = [0, 0, 0, 0]): PadLike {
  return {
    index: 0, id: '8BitDo Ultimate 2 (STANDARD GAMEPAD Vendor: 2dc8 Product: 6012)', connected: true, mapping: 'standard',
    buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: (pressed[i] ?? 0) > 0.5, value: pressed[i] ?? 0 })), axes, ...over,
  };
}

describe('padView', () => {
  it('reads pressed buttons, analog triggers and sticks', () => {
    const v = padView(pad({}, { 0: 1, 6: 0.5 }, [0.6, -0.3, 0, 0.04]), 0.08);
    expect(v.buttons[0]).toMatchObject({ pressed: true, label: 'A' });
    expect(v.buttons[1].pressed).toBe(false);
    expect(v.lt).toBe(0.5);
    expect(v.rt).toBe(0);
    expect(v.left).toMatchObject({ x: 0.6, y: -0.3, inDeadzone: false });
    expect(v.left.pctX).toBe(60);
    expect(v.right.inDeadzone).toBe(true);
  });
  it('tolerates short axes/buttons and NaN', () => {
    const v = padView(pad({ buttons: [], axes: [NaN] }), DEFAULT_DEADZONE);
    expect(v.buttons.length).toBe(17);
    expect(v.left.x).toBe(0);
    expect(v.right.mag).toBe(0);
  });
});

describe('stickView deadzone', () => {
  it('is inside up to and including the ring', () => {
    expect(stickView(0.08, 0, 0.08).inDeadzone).toBe(true);
    expect(stickView(0.081, 0, 0.08).inDeadzone).toBe(false);
    expect(DEFAULT_DEADZONE).toBe(0.08);
  });
});

describe('drift verdict', () => {
  it('thresholds: <2 OK, 2..6 Minor, >6 Drift', () => {
    expect(driftVerdict(0)).toBe('OK');
    expect(driftVerdict(1.99)).toBe('OK');
    expect(driftVerdict(2)).toBe('Minor');
    expect(driftVerdict(6)).toBe('Minor');
    expect(driftVerdict(6.01)).toBe('Drift');
  });
  it('suggests offset + noise rounded up', () => {
    expect(suggestDeadzone(4.4, 1.2)).toBe(6);
    expect(suggestDeadzone(0, 0)).toBe(0);
  });
});

describe('DriftCheck', () => {
  const rest = (x = 0, y = 0) => padView(pad({}, {}, [x, y, 0, 0]), 0.08);
  it('reports mean, peak-to-peak and verdict after the duration', () => {
    const c = new DriftCheck(3000);
    let r = c.push(rest(0.04, 0.02), 0);
    expect(r.state).toBe('running');
    c.push(rest(0.05, 0.02), 1000);
    c.push(rest(0.03, 0.02), 2000);
    r = c.push(rest(0.04, 0.02), 3000);
    expect(r.state).toBe('done');
    if (r.state !== 'done') return;
    expect(r.left.meanX).toBeCloseTo(4, 5);
    expect(r.left.meanY).toBeCloseTo(2, 5);
    expect(r.left.offset).toBeCloseTo(Math.hypot(4, 2), 5);
    expect(r.left.noise).toBeCloseTo(2, 5);
    expect(r.left.verdict).toBe('Minor');
    expect(r.left.suggested).toBe(Math.ceil(Math.hypot(4, 2) + 2));
    expect(r.right.verdict).toBe('OK');
    expect(r.lt.rest).toBe(0);
  });
  it('cancels when a stick, trigger or button moves a lot', () => {
    for (const v of [rest(0.6, 0), padView(pad({}, { 6: 0.8 }), 0.08), padView(pad({}, { 0: 1 }), 0.08)]) {
      const c = new DriftCheck(3000);
      c.push(rest(), 0);
      expect(c.push(v, 500).state).toBe('moved');
    }
  });
});

describe('RangeTest', () => {
  it('bins angles and keeps the max extent', () => {
    const r = new RangeTest(32);
    expect(r.bin(1, 0)).toBe(0);
    expect(r.bin(0, 1)).toBe(8);
    expect(r.bin(-1, 0)).toBe(16);
    expect(r.bin(0, -1)).toBe(24);
    r.record(1, 0);
    r.record(0.7, 0);
    r.record(0.01, 0.01); // near centre: ignored
    expect(r.max[0]).toBe(1);
    expect(r.max.filter((m) => m > 0).length).toBe(1);
    r.reset();
    expect(r.max.every((m) => m === 0)).toBe(true);
  });
  it('polygon has one point per bin, radius = max extent', () => {
    const r = new RangeTest(4);
    r.record(1, 0);
    const p = r.polygon();
    expect(p.length).toBe(4);
    expect(Math.hypot(...p[0])).toBeCloseTo(1, 1);
  });
});

describe('PollRate', () => {
  it('estimates Hz over a rolling window', () => {
    const p = new PollRate(1000);
    expect(p.hz).toBe(0);
    for (let t = 0; t <= 2000; t += 10) p.tick(t);
    expect(p.hz).toBeCloseTo(100, 0);
    for (let t = 2016; t <= 4000; t += 16) p.tick(t);
    expect(p.hz).toBeCloseTo(62.5, 0);
  });
});

describe('Trail', () => {
  it('drops points older than 2 s', () => {
    const t = new Trail(2000);
    t.add(0, 0, 0);
    t.add(0.1, 0.1, 1500);
    t.add(0.2, 0.2, 2600);
    expect(t.points.length).toBe(2);
  });
});

describe('padChoices', () => {
  it('lists connected pads with details and skips holes', () => {
    const c = padChoices([null, pad({ index: 1 }), pad({ index: 3, connected: false }), pad({ index: 2, mapping: '', axes: [0, 0] })]);
    expect(c.map((x) => x.index)).toEqual([1, 2]);
    expect(c[0].detail).toContain('standard');
    expect(c[0].detail).toContain('17 buttons');
    expect(c[0].detail).toContain('4 axes');
    expect(c[1].detail).toContain('2 axes');
  });
});

describe('mappingRows agree with gamepad.ts', () => {
  const rows = mappingRows();
  it('has a row per button flag', () => {
    BUTTON_FLAGS.forEach((f, i) => {
      const r = rows.find((x) => x.kind === 'button' && x.index === i)!;
      expect(r).toBeTruthy();
      if (f) expect(r.sends).toBe('0x' + f.toString(16).padStart(4, '0'));
      else expect(r.sends).toBe('analog u8');
    });
  });
  it('has four stick axes as i16, Y inverted', () => {
    const ax = rows.filter((x) => x.kind === 'axis');
    expect(ax.map((x) => x.index)).toEqual([0, 1, 2, 3]);
    expect(ax.every((x) => x.sends.startsWith('i16'))).toBe(true);
    expect(ax[1].sends).toMatch(/inverted/);
    expect(ax[3].sends).toMatch(/inverted/);
  });
});
