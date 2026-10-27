// Pure logic of Settings' "Controller test" panel (no DOM, no network; nothing is sent to the host).
// What Brisk sends comes from gamepad.ts (BUTTON_FLAGS); this file only labels and measures.
import { BUTTON_FLAGS, type PadLike } from '../../lib/gamepad';

export const DEFAULT_DEADZONE = 0.08;
/** Standard-mapping button index -> physical label, same order as BUTTON_FLAGS. */
export const BUTTON_LABELS = [
  'A',
  'B',
  'X',
  'Y',
  'LB',
  'RB',
  'LT',
  'RT',
  'Back',
  'Start',
  'L3',
  'R3',
  'D-pad up',
  'D-pad down',
  'D-pad left',
  'D-pad right',
  'Home',
];
export const AXIS_LABELS = ['Left stick X', 'Left stick Y', 'Right stick X', 'Right stick Y'];

const fin = (v: number | undefined) => (Number.isFinite(v) ? (v as number) : 0);
const clamp = (v: number) => Math.min(1, Math.max(-1, v));

export type StickView = { x: number; y: number; mag: number; pctX: number; pctY: number; pctMag: number; inDeadzone: boolean };
export type PadView = {
  buttons: { index: number; label: string; pressed: boolean; value: number }[];
  lt: number;
  rt: number;
  left: StickView;
  right: StickView;
};

export function stickView(x: number, y: number, deadzone: number): StickView {
  const mag = Math.hypot(x, y);
  return { x, y, mag, pctX: Math.round(x * 100), pctY: Math.round(y * 100), pctMag: Math.round(mag * 100), inDeadzone: mag <= deadzone };
}

export function padView(g: PadLike, deadzone: number): PadView {
  const buttons = BUTTON_LABELS.map((label, index) => ({
    index,
    label,
    pressed: !!g.buttons[index]?.pressed,
    value: fin(g.buttons[index]?.value),
  }));
  const a = (i: number) => clamp(fin(g.axes[i]));
  return {
    buttons,
    lt: buttons[6].value,
    rt: buttons[7].value,
    left: stickView(a(0), a(1), deadzone),
    right: stickView(a(2), a(3), deadzone),
  };
}

export const padChoices = (pads: readonly (PadLike | null)[]) =>
  pads.flatMap((g) =>
    g?.connected
      ? [{ index: g.index, id: g.id, detail: `${g.mapping || 'no mapping'}, ${g.buttons.length} buttons, ${g.axes.length} axes` }]
      : [],
  );

// --- drift ---
export type Verdict = 'OK' | 'Minor' | 'Drift';
export const driftVerdict = (offsetPct: number): Verdict => (offsetPct < 2 ? 'OK' : offsetPct <= 6 ? 'Minor' : 'Drift');
export const suggestDeadzone = (offsetPct: number, noisePct: number) => Math.max(0, Math.ceil(offsetPct + noisePct - 1e-9)); // epsilon: 4.0000000001 must not become 5

export type StickDrift = { meanX: number; meanY: number; offset: number; noise: number; verdict: Verdict; suggested: number };
export type DriftResult =
  | { state: 'running' }
  | { state: 'moved' }
  | { state: 'done'; left: StickDrift; right: StickDrift; lt: { rest: number }; rt: { rest: number } };

const MOVED_STICK = 0.3; // beyond any drift: the user is touching it
const MOVED_TRIGGER = 0.3;
const p2p = (v: number[]) => (Math.max(...v) - Math.min(...v)) * 100;
const mean = (v: number[]) => v.reduce((s, x) => s + x, 0) / v.length;

export class DriftCheck {
  private start: number | null = null;
  private s = { lx: [] as number[], ly: [] as number[], rx: [] as number[], ry: [] as number[], lt: [] as number[], rt: [] as number[] };
  constructor(private durationMs = 3000) {}

  push(v: PadView, now: number): DriftResult {
    if (
      v.buttons.some((b) => b.pressed && b.index !== 6 && b.index !== 7) ||
      v.lt > MOVED_TRIGGER ||
      v.rt > MOVED_TRIGGER ||
      v.left.mag > MOVED_STICK ||
      v.right.mag > MOVED_STICK
    )
      return { state: 'moved' };
    this.start ??= now;
    this.s.lx.push(v.left.x);
    this.s.ly.push(v.left.y);
    this.s.rx.push(v.right.x);
    this.s.ry.push(v.right.y);
    this.s.lt.push(v.lt);
    this.s.rt.push(v.rt);
    if (now - this.start < this.durationMs) return { state: 'running' };
    return {
      state: 'done',
      left: this.stick(this.s.lx, this.s.ly),
      right: this.stick(this.s.rx, this.s.ry),
      lt: { rest: mean(this.s.lt) },
      rt: { rest: mean(this.s.rt) },
    };
  }

  private stick(xs: number[], ys: number[]): StickDrift {
    const meanX = mean(xs) * 100,
      meanY = mean(ys) * 100;
    const offset = Math.hypot(meanX, meanY),
      noise = Math.max(p2p(xs), p2p(ys));
    return { meanX, meanY, offset, noise, verdict: driftVerdict(offset), suggested: suggestDeadzone(offset, noise) };
  }
}

// --- range test ---
export class RangeTest {
  max: number[];
  constructor(private bins = 32) {
    this.max = Array(bins).fill(0);
  }
  bin(x: number, y: number) {
    const a = (Math.atan2(y, x) + 2 * Math.PI) % (2 * Math.PI);
    return Math.min(this.bins - 1, Math.floor((a / (2 * Math.PI)) * this.bins));
  }
  record(x: number, y: number) {
    const m = Math.hypot(x, y);
    if (m < 0.1) return; // near centre has no direction
    const b = this.bin(x, y);
    if (m > this.max[b]) this.max[b] = m;
  }
  reset() {
    this.max.fill(0);
  }
  /** One point per bin at its centre angle, radius = max extent (axes' own orientation: y down). */
  polygon(): [number, number][] {
    return this.max.map((m, i) => {
      const a = ((i + 0.5) / this.bins) * 2 * Math.PI;
      return [m * Math.cos(a), m * Math.sin(a)];
    });
  }
}

// --- timing ---
export class PollRate {
  private t: number[] = [];
  hz = 0;
  constructor(private windowMs = 1000) {}
  tick(now: number) {
    this.t.push(now);
    while (this.t.length > 1 && now - this.t[0] > this.windowMs) this.t.shift();
    const span = now - this.t[0];
    this.hz = this.t.length > 1 && span > 0 ? ((this.t.length - 1) * 1000) / span : 0;
  }
}

export class Trail {
  points: { x: number; y: number; t: number }[] = [];
  constructor(private ms = 2000) {}
  add(x: number, y: number, now: number) {
    this.points.push({ x, y, t: now });
    while (this.points.length && now - this.points[0].t > this.ms) this.points.shift();
  }
}

// --- mapping table ---
export type MapRow = { kind: 'button' | 'axis'; index: number; label: string; sends: string };
export function mappingRows(): MapRow[] {
  const buttons = BUTTON_FLAGS.map(
    (f, index): MapRow => ({
      kind: 'button',
      index,
      label: BUTTON_LABELS[index],
      sends: f ? '0x' + f.toString(16).padStart(4, '0') : 'analog u8',
    }),
  );
  const axes = AXIS_LABELS.map(
    (label, index): MapRow => ({ kind: 'axis', index, label, sends: index % 2 ? 'i16 (Y inverted: up is positive)' : 'i16' }),
  );
  return [...buttons, ...axes];
}
