import { describe, expect, it } from 'vitest';
import type { PadLike } from '../../../lib/gamepad';
import { padView } from '../../settings/controllertest';
import { Listener, wrongKind } from './listen';

/** A pad frame: `down` maps button index -> value, `axes` is [lx, ly, rx, ry]. */
const frame = (down: Record<number, number> = {}, axes = [0, 0, 0, 0]) =>
  padView(
    {
      index: 0,
      id: 'pad',
      connected: true,
      mapping: 'standard',
      buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: (down[i] ?? 0) > 0.5, value: down[i] ?? 0 })),
      axes,
    } satisfies PadLike,
    0.08,
  );
const feed = (l: Listener, ...fs: ReturnType<typeof frame>[]) => fs.map((f) => l.push(f));

describe('Listener', () => {
  it('the first new press wins', () => {
    const l = new Listener('a');
    expect(feed(l, frame(), frame(), frame({ 2: 1 }), frame({ 2: 1 }))).toEqual([
      { type: 'none' },
      { type: 'none' },
      { type: 'bind', pad: 'X' },
      { type: 'none' }, // still held: not a new press
    ]);
  });

  it('a button held when listening starts is ignored until released once', () => {
    const l = new Listener('a');
    const r = feed(l, frame({ 0: 1 }), frame({ 0: 1 }), frame(), frame({ 0: 1 }));
    expect(r.map((x) => x.type)).toEqual(['none', 'none', 'none', 'bind']);
    expect(r[3]).toEqual({ type: 'bind', pad: 'A' });
  });

  it('with two new buttons in one frame the lowest index wins', () => {
    const l = new Listener('a');
    expect(feed(l, frame(), frame({ 5: 1, 1: 1 }))[1]).toEqual({ type: 'bind', pad: 'B' });
  });

  it('triggers count above 0.5', () => {
    const l = new Listener('a');
    expect(feed(l, frame(), frame({ 6: 0.4 }), frame({ 6: 0.6 }))).toEqual([
      { type: 'none' },
      { type: 'none' },
      { type: 'bind', pad: 'LT' },
    ]);
  });

  it('answers the d-pad, Guide, and nothing for stick clicks', () => {
    expect(feed(new Listener('dp'), frame(), frame({ 14: 1 }))[1]).toEqual({ type: 'bind', pad: 'D-pad' });
    expect(feed(new Listener('home'), frame(), frame({ 16: 1 }))[1]).toEqual({ type: 'bind', pad: 'Guide' });
    expect(feed(new Listener('a'), frame(), frame({ 10: 1, 11: 1 }))[1]).toEqual({ type: 'none' });
  });

  it('a stick answers past 0.6 and must come back below 0.3 to answer again', () => {
    const l = new Listener('ls');
    const r = feed(
      l,
      frame(),
      frame({}, [0.5, 0, 0, 0]), // not far enough
      frame({}, [0.7, 0, 0, 0]),
      frame({}, [0.4, 0, 0, 0]), // between the two thresholds: still the same push
      frame({}, [0.7, 0, 0, 0]),
      frame({}, [0.1, 0, 0, 0]),
      frame({}, [0, 0, 0, -0.9]),
    );
    expect(r.map((x) => (x.type === 'bind' ? x.pad : x.type))).toEqual([
      'none',
      'none',
      'Left stick',
      'none',
      'none',
      'none',
      'Right stick',
    ]);
  });

  it('a stick already pushed at the start is ignored until released', () => {
    const l = new Listener('ls');
    const r = feed(l, frame({}, [0.9, 0, 0, 0]), frame({}, [0.9, 0, 0, 0]), frame(), frame({}, [0.9, 0, 0, 0]));
    expect(r.map((x) => x.type)).toEqual(['none', 'none', 'none', 'bind']);
  });

  it('the wrong kind is ignored with a hint and listening goes on', () => {
    const l = new Listener('a');
    const r = feed(l, frame(), frame({}, [0.9, 0, 0, 0]), frame(), frame({ 0: 1 }));
    expect(r[1]).toEqual({ type: 'wrong', pad: 'Left stick', hint: 'That is a stick. A takes a button.' });
    expect(r[3]).toEqual({ type: 'bind', pad: 'A' });
    expect(wrongKind('ls', 'A')).toBe('That is a button. Left stick takes a stick.');
    expect(wrongKind('dp', 'A')).toBe('That is a button. D-pad takes the D-pad.');
    expect(wrongKind('a', 'D-pad')).toBe('That is the D-pad. A takes a button.');
  });

  it('waits with no pad, cancels when a pad that was there goes, and takes its baseline when one shows up', () => {
    const l = new Listener('a');
    expect(l.push(null)).toEqual({ type: 'none' });
    expect(l.push(frame({ 0: 1 }))).toEqual({ type: 'none' }); // held on arrival: baseline
    expect(l.push(frame({ 0: 1 }))).toEqual({ type: 'none' });
    expect(l.push(null)).toEqual({ type: 'cancel' });
  });
});
