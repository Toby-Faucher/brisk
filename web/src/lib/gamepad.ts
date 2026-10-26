// Brisk gamepads: Gamepad API -> brisk-bridge -> Sunshine, rumble back. Framework-free.
//
// Same stream as keys and mouse (input.ts): frames `len u8, kind u8, fields`, little-endian
// (docs/controller-notes.md). Each pad sends its whole state, so the newest frame always tells the
// truth: on every change, again every 100 ms while anything is off neutral and every 250 ms
// idle. The bridge drops repeats and keeps every button change; the resend also keeps its 1 s
// held-input watchdog quiet. The Gamepad API only updates for a focused page, so pads are polled
// (4 ms timer, rAF as a backup) only while the document is focused and visible; leaving releases
// every pad (neutral, still attached). pagehide, a pad's disconnect and the transport closing
// detach it (neutral, then removal: Sunshine frees its virtual pad).
//
// No deadzone here: Sunshine passes values straight to its virtual pad (virtualhid_input.cpp
// normalize_axis), like a real pad, and games apply their own.

export const KIND_GAMEPAD = 0x15;
export const KIND_GAMEPAD_GONE = 0x16;
/// Bridge to page: `slot u8, low u16, high u16, left u16, right u16` (motor values, 0 = stop).
export const KIND_RUMBLE = 0x20;
export const MAX_PADS = 4;
export const POLL_MS = 4;
export const ACTIVE_RESEND_MS = 100;
export const IDLE_RESEND_MS = 250;
/// One effect's length; a rumble still on is replayed every RUMBLE_REFRESH_MS, so a lost page
/// stops shaking within a second.
export const RUMBLE_MS = 1000;
export const RUMBLE_REFRESH_MS = 500;

/// Standard-mapping button index -> Moonlight buttonFlags (6 and 7 are the analog triggers).
export const BUTTON_FLAGS = [
  0x1000, 0x2000, 0x4000, 0x8000, // A B X Y
  0x0100, 0x0200, 0, 0, // LB RB LT RT
  0x0020, 0x0010, 0x0040, 0x0080, // Back Start L3 R3
  0x0001, 0x0002, 0x0004, 0x0008, // up down left right
  0x0400, // Home / Guide
];

export type PadState = { buttons: number; lt: number; rt: number; lx: number; ly: number; rx: number; ry: number };
const NEUTRAL: PadState = { buttons: 0, lt: 0, rt: 0, lx: 0, ly: 0, rx: 0, ry: 0 };

type Actuator = {
  effects?: readonly string[];
  playEffect(type: string, params: Record<string, number>): Promise<unknown>;
  reset(): Promise<unknown>;
};
/// The parts of `Gamepad` used here (tests pass plain objects).
export type PadLike = {
  index: number;
  id: string;
  connected: boolean;
  mapping: string;
  buttons: readonly { pressed: boolean; value: number }[];
  axes: readonly number[];
  vibrationActuator?: Actuator | null;
};
export type PadInfo = { slot: number; name: string };

const num = (v: number | undefined) => (Number.isFinite(v) ? (v as number) : 0);
const trigger = (v: number | undefined) => Math.round(Math.min(1, Math.max(0, num(v))) * 255);
const axis = (v: number | undefined) => {
  const x = Math.min(1, Math.max(-1, num(v)));
  return Math.round(x < 0 ? x * 32768 : x * 32767) || 0; // no -0
};

export function readPad(g: PadLike): PadState {
  let buttons = 0;
  BUTTON_FLAGS.forEach((f, i) => g.buttons[i]?.pressed && (buttons |= f));
  const a = g.axes;
  return { buttons, lt: trigger(g.buttons[6]?.value), rt: trigger(g.buttons[7]?.value), lx: axis(a[0]), ly: axis(-num(a[1])), rx: axis(a[2]), ry: axis(-num(a[3])) };
}

export function padFrame(slot: number, s: PadState) {
  const f = new Uint8Array(15);
  const v = new DataView(f.buffer);
  f.set([14, KIND_GAMEPAD, slot]);
  v.setUint16(3, s.buttons, true);
  f[5] = s.lt;
  f[6] = s.rt;
  [s.lx, s.ly, s.rx, s.ry].forEach((x, i) => v.setInt16(7 + 2 * i, Math.max(-32768, Math.min(32767, x)), true));
  return f;
}
export const goneFrame = (slot: number) => Uint8Array.of(2, KIND_GAMEPAD_GONE, slot);

/// `Gamepad.id` without Chrome's "(STANDARD GAMEPAD Vendor: ...)" tail.
export const padName = (id: string) => id.replace(/\s*\(.*$/, '').replace(/\s*Vendor:.*$/i, '').trim() || 'Controller';

const same = (a: PadState, b: PadState) => (Object.keys(a) as (keyof PadState)[]).every((k) => a[k] === b[k]);
const neutral = (s: PadState) => same(s, NEUTRAL);

type Slot = { index: number; name: string; sent: PadState; at: number; rumble: number[]; rumbleAt: number };

export class GamepadSender {
  private slots: (Slot | null)[] = Array(MAX_PADS).fill(null);

  constructor(
    private write: (f: Uint8Array) => void,
    private getGamepads: () => readonly (PadLike | null)[],
    private onpads: (p: PadInfo[]) => void = () => {},
  ) {}

  /// Reads every pad and sends what changed or is due again.
  poll() {
    const now = Date.now();
    const live = (this.getGamepads() ?? []).filter((g): g is PadLike => !!g && g.connected && g.mapping === 'standard');
    let changed = false;
    this.slots.forEach((s, i) => {
      if (s && !live.some((g) => g.index === s.index)) changed = this.drop(i) || changed;
    });
    for (const g of live) {
      let i = this.slots.findIndex((s) => s?.index === g.index);
      if (i < 0) {
        i = this.slots.indexOf(null);
        if (i < 0) continue; // a fifth pad: ignored
        this.slots[i] = { index: g.index, name: padName(g.id), sent: NEUTRAL, at: -Infinity, rumble: [0, 0, 0, 0], rumbleAt: 0 };
        changed = true;
      }
      const s = this.slots[i]!;
      const st = readPad(g);
      if (!same(st, s.sent) || s.at === -Infinity || now - s.at >= (neutral(st) ? IDLE_RESEND_MS : ACTIVE_RESEND_MS)) this.send(i, st, now);
      if (s.rumble.some((v) => v) && now - s.rumbleAt >= RUMBLE_REFRESH_MS) this.play(i, g);
    }
    if (changed) this.report();
  }

  /// Neutral for every pad off neutral (focus lost: nothing must stay pressed); still attached.
  release() {
    this.slots.forEach((s, i) => {
      if (!s) return;
      if (!neutral(s.sent)) this.send(i, NEUTRAL, Date.now());
      if (s.rumbleAt === 0) return;
      this.pad(i)?.vibrationActuator?.reset().catch(() => {});
      s.rumbleAt = 0; // replayed by the next focused poll if still on
    });
  }

  /// The browser says gamepad `index` is gone.
  disconnected(index: number) {
    const i = this.slots.findIndex((s) => s?.index === index);
    if (i >= 0 && this.drop(i)) this.report();
  }

  /// Every pad gone (pagehide, stream stop, transport closed).
  detachAll() {
    let changed = false;
    this.slots.forEach((_, i) => (changed = this.drop(i) || changed));
    if (changed) this.report();
  }

  /// Motor values from Sunshine for `slot` (u16 each; 0 everywhere = stop).
  rumble(slot: number, low: number, high: number, left: number, right: number) {
    const s = this.slots[slot];
    if (!s) return;
    s.rumble = [low, high, left, right];
    const g = this.pad(slot);
    if (g) this.play(slot, g);
  }

  private play(i: number, g: PadLike) {
    const s = this.slots[i]!;
    const act = g.vibrationActuator;
    s.rumbleAt = Date.now();
    const [low, high, left, right] = s.rumble.map((v) => v / 0xffff);
    if (!(low || high || left || right)) s.rumbleAt = 0;
    if (!act) return;
    const base = { duration: RUMBLE_MS, startDelay: 0, strongMagnitude: low, weakMagnitude: high };
    const triggers = (left || right) && act.effects?.includes('trigger-rumble');
    if (triggers) act.playEffect('trigger-rumble', { ...base, leftTrigger: left, rightTrigger: right }).catch(() => {});
    else if (low || high) act.playEffect('dual-rumble', base).catch(() => {});
    else act.reset().catch(() => {});
  }

  private pad(i: number) {
    const s = this.slots[i];
    return s ? (this.getGamepads() ?? []).find((g) => g?.index === s.index) ?? null : null;
  }

  private send(i: number, st: PadState, now: number) {
    const s = this.slots[i]!;
    s.sent = st;
    s.at = now;
    this.write(padFrame(i, st));
  }

  /// Neutral (if needed), removal, rumble off. False if the slot was empty.
  private drop(i: number) {
    const s = this.slots[i];
    if (!s) return false;
    if (!neutral(s.sent)) this.write(padFrame(i, NEUTRAL));
    this.write(goneFrame(i));
    if (s.rumble.some((v) => v)) this.pad(i)?.vibrationActuator?.reset().catch(() => {});
    this.slots[i] = null;
    return true;
  }

  private report() {
    this.onpads(this.slots.flatMap((s, slot) => (s ? [{ slot, name: s.name }] : [])));
  }
}

/// Splits the bridge's byte stream back into frame bodies (`len u8` + body).
export class FrameSplitter {
  private buf = new Uint8Array(0);

  push(chunk: Uint8Array): Uint8Array[] {
    const b = new Uint8Array(this.buf.length + chunk.length);
    b.set(this.buf);
    b.set(chunk, this.buf.length);
    const out: Uint8Array[] = [];
    let at = 0;
    while (at < b.length && at + 1 + b[at] <= b.length) {
      out.push(b.slice(at + 1, at + 1 + b[at]));
      at += 1 + b[at];
    }
    this.buf = b.slice(at);
    return out;
  }
}

/// Applies one bridge frame body to `s` (unknown kinds are ignored).
export function onBridgeFrame(s: GamepadSender, b: Uint8Array) {
  if (b[0] !== KIND_RUMBLE || b.length !== 10) return;
  const v = new DataView(b.buffer, b.byteOffset);
  s.rumble(b[1], v.getUint16(2, true), v.getUint16(4, true), v.getUint16(6, true), v.getUint16(8, true));
}

export interface GamepadEnv {
  doc: EventTarget & { hasFocus(): boolean; visibilityState: string };
  win: EventTarget;
  raf: (f: () => void) => number;
  caf: (h: number) => void;
}

/// Polls pads into `s` until `closed` settles, then detaches them.
export function attachGamepads(s: GamepadSender, { doc, win, raf, caf }: GamepadEnv, closed: Promise<unknown>) {
  const focused = () => doc.hasFocus() && doc.visibilityState === 'visible';
  const tick = () => (focused() ? s.poll() : s.release());
  // Chrome clamps timers in a hidden tab, but nothing is polled there anyway.
  const timer = setInterval(tick, POLL_MS);
  let frame = 0;
  const loop = () => {
    tick();
    frame = raf(loop);
  };
  frame = raf(loop);
  const gone = new AbortController();
  const on = (t: EventTarget, type: string, f: (e: any) => void) => t.addEventListener(type, f, { signal: gone.signal });
  on(win, 'blur', () => s.release());
  on(doc, 'visibilitychange', () => doc.visibilityState === 'hidden' && s.release());
  on(win, 'pagehide', () => s.detachAll());
  on(win, 'gamepaddisconnected', (e: { gamepad: { index: number } }) => s.disconnected(e.gamepad.index));
  closed.finally(() => {
    clearInterval(timer);
    caf(frame);
    gone.abort();
    s.detachAll();
  });
}
