// Brisk input: keyboard and mouse -> brisk-bridge -> Sunshine. Framework-free.
//
// One WebTransport bidirectional stream (one stream keeps key-down before key-up), carrying
// frames `len u8, kind u8, fields` (little-endian; docs/m2-notes.md "Input"). The bridge maps
// them to Moonlight input packets. Keys go by `KeyboardEvent.code` (the physical key, so the
// layout is the host's), as Windows virtual-key codes. The mouse is relative, under Pointer Lock.
//
// Capture: a click on the canvas focuses it and asks for Pointer Lock. Keys are sent while the
// canvas has focus; motion, buttons and wheel only while locked. Escape is never sent: Chrome
// leaves Pointer Lock on Escape itself, and the page then also drops focus, so Escape always
// gets the user out. Everything held is released on blur, a hidden tab, pagehide, lock loss and
// transport close (the bridge also releases what is held when its stream ends).
//
// Gamepads (gamepad.ts) write on the same stream; the bridge sends rumble back on it.

import { listener } from './listen';
import { FrameSplitter, GamepadSender, attachGamepads, onBridgeFrame, type PadInfo } from './gamepad';

export const KIND_KEY = 0x10;
export const KIND_MOVE = 0x11;
export const KIND_BUTTON = 0x12;
export const KIND_SCROLL = 0x13;
/// Motion and wheel are summed and sent at most once per this many ms (a 1 kHz mouse becomes
/// 125 messages a second).
export const FLUSH_MS = 8;
/// While a key or button is held, a heartbeat goes out this often; the bridge releases
/// everything after 1 s without any frame (a lost network or a frozen tab).
export const HEARTBEAT_MS = 250;
const KIND_HEARTBEAT = 0x14;
export const heartbeatFrame = Uint8Array.of(1, KIND_HEARTBEAT);
/// Moonlight's one wheel notch.
const WHEEL_DELTA = 120;

function table(): Record<string, number> {
  const t: Record<string, number> = {};
  for (let i = 0; i < 26; i++) t[`Key${String.fromCharCode(65 + i)}`] = 0x41 + i;
  for (let i = 0; i < 10; i++) {
    t[`Digit${i}`] = 0x30 + i;
    t[`Numpad${i}`] = 0x60 + i;
  }
  for (let i = 1; i <= 24; i++) t[`F${i}`] = 0x6f + i;
  return Object.assign(t, {
    Backspace: 0x08,
    Tab: 0x09,
    Enter: 0x0d,
    NumpadEnter: 0x0d,
    Pause: 0x13,
    CapsLock: 0x14,
    Escape: 0x1b,
    Space: 0x20,
    PageUp: 0x21,
    PageDown: 0x22,
    End: 0x23,
    Home: 0x24,
    ArrowLeft: 0x25,
    ArrowUp: 0x26,
    ArrowRight: 0x27,
    ArrowDown: 0x28,
    PrintScreen: 0x2c,
    Insert: 0x2d,
    Delete: 0x2e,
    MetaLeft: 0x5b,
    MetaRight: 0x5c,
    ContextMenu: 0x5d,
    NumpadMultiply: 0x6a,
    NumpadAdd: 0x6b,
    NumpadSubtract: 0x6d,
    NumpadDecimal: 0x6e,
    NumpadDivide: 0x6f,
    NumLock: 0x90,
    ScrollLock: 0x91,
    ShiftLeft: 0xa0,
    ShiftRight: 0xa1,
    ControlLeft: 0xa2,
    ControlRight: 0xa3,
    AltLeft: 0xa4,
    AltRight: 0xa5,
    Semicolon: 0xba,
    Equal: 0xbb,
    Comma: 0xbc,
    Minus: 0xbd,
    Period: 0xbe,
    Slash: 0xbf,
    Backquote: 0xc0,
    BracketLeft: 0xdb,
    Backslash: 0xdc,
    BracketRight: 0xdd,
    Quote: 0xde,
    IntlBackslash: 0xe2,
  });
}

/// `KeyboardEvent.code` -> Windows virtual-key code (what Moonlight sends; left/right modifiers
/// distinct, like moonlight-qt).
export const CODE_TO_VK: Record<string, number> = table();

export const keyFrame = (vk: number, down: boolean, mods: number) => Uint8Array.of(4, KIND_KEY, vk, down ? 1 : 0, mods);
export const buttonFrame = (button: number, down: boolean) => Uint8Array.of(3, KIND_BUTTON, button, down ? 1 : 0);
export function moveFrame(dx: number, dy: number) {
  const f = Uint8Array.of(5, KIND_MOVE, 0, 0, 0, 0);
  const v = new DataView(f.buffer);
  v.setInt16(2, dx, true);
  v.setInt16(4, dy, true);
  return f;
}
export function scrollFrame(amount: number) {
  const f = Uint8Array.of(3, KIND_SCROLL, 0, 0);
  new DataView(f.buffer).setInt16(2, amount, true);
  return f;
}

export const modifiers = (e: { shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean }) =>
  (e.shiftKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.altKey ? 4 : 0) | (e.metaKey ? 8 : 0);

/// A wheel event's deltaY in Moonlight units (120 per notch, positive = up). Chrome reports
/// 100 px per notch; a line is a third of a notch.
export function wheelUnits(deltaY: number, deltaMode: number): number {
  const notches = deltaMode === 0 ? deltaY / 100 : deltaMode === 1 ? deltaY / 3 : deltaY;
  return Math.round(-notches * WHEEL_DELTA);
}

const clamp16 = (x: number) => Math.max(-32768, Math.min(32767, x));

export interface InputStats {
  keys: number;
  buttons: number;
  moves: number;
  scrolls: number;
  /// Motion or wheel the stream would not take (its buffer was full).
  dropped: number;
  releaseAlls: number;
}

/// Turns input into frames for `write`; `droppable` frames (motion, wheel) may be refused
/// (returns false) when the stream is backed up. Keys and buttons are always written.
export class InputSender {
  stats: InputStats = { keys: 0, buttons: 0, moves: 0, scrolls: 0, dropped: 0, releaseAlls: 0 };
  private keys = new Set<number>();
  private buttons = new Set<number>();
  private dx = 0;
  private dy = 0;
  private wheel_ = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private beat: ReturnType<typeof setInterval> | null = null;

  constructor(private write: (f: Uint8Array, droppable: boolean) => boolean) {}

  key(vk: number, down: boolean, mods: number) {
    if (this.keys.has(vk) === down) return; // a repeat, or a release of a key we never sent
    this.flush();
    if (down) this.keys.add(vk);
    else this.keys.delete(vk);
    this.stats.keys++;
    this.write(keyFrame(vk, down, mods), false);
    this.heartbeat();
  }

  button(button: number, down: boolean) {
    if (this.buttons.has(button) === down) return;
    this.flush();
    if (down) this.buttons.add(button);
    else this.buttons.delete(button);
    this.stats.buttons++;
    this.write(buttonFrame(button, down), false);
    this.heartbeat();
  }

  /// Heartbeats run exactly while something is held.
  private heartbeat() {
    const held = this.keys.size + this.buttons.size > 0;
    if (held && this.beat === null) this.beat = setInterval(() => this.write(heartbeatFrame, false), HEARTBEAT_MS);
    else if (!held && this.beat !== null) {
      clearInterval(this.beat);
      this.beat = null;
    }
  }

  move(dx: number, dy: number) {
    this.dx += dx;
    this.dy += dy;
    this.schedule();
  }

  wheel(units: number) {
    this.wheel_ += units;
    this.schedule();
  }

  private schedule() {
    this.timer ??= setTimeout(() => this.flush(), FLUSH_MS);
  }

  /// Sends the summed motion and wheel now.
  flush() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    while (this.dx !== 0 || this.dy !== 0) {
      const [x, y] = [clamp16(this.dx), clamp16(this.dy)];
      this.dx -= x;
      this.dy -= y;
      this.send(moveFrame(x, y), 'moves');
    }
    if (this.wheel_ !== 0) {
      this.send(scrollFrame(clamp16(this.wheel_)), 'scrolls');
      this.wheel_ = 0;
    }
  }

  private send(f: Uint8Array, what: 'moves' | 'scrolls') {
    if (this.write(f, true)) this.stats[what]++;
    else this.stats.dropped++;
  }

  /// Releases every held key and button; pending motion is dropped.
  releaseAll() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    this.dx = this.dy = this.wheel_ = 0;
    if (this.keys.size || this.buttons.size) this.stats.releaseAlls++;
    for (const vk of this.keys) this.write(keyFrame(vk, false, 0), false);
    for (const b of this.buttons) this.write(buttonFrame(b, false), false);
    this.keys.clear();
    this.buttons.clear();
    this.heartbeat();
  }
}

/// What `attach` needs of the page (real DOM objects in the page, plain EventTargets in tests).
export interface InputEnv {
  canvas: EventTarget & { requestPointerLock(): unknown; focus(): void; blur(): void };
  doc: EventTarget & { pointerLockElement: unknown; activeElement: unknown; visibilityState: string };
  win: EventTarget;
}

/// Wires page events to `s` until `closed` settles (then removes its listeners).
export function attach(s: InputSender, { canvas, doc, win }: InputEnv, closed: Promise<unknown>) {
  const locked = () => doc.pointerLockElement === canvas;
  const captured = () => locked() || doc.activeElement === canvas;
  const leave = () => {
    s.releaseAll();
    if (doc.activeElement === canvas) canvas.blur();
  };
  // Every listener goes when `closed` settles, so a reconnect does not leave the old ones behind.
  const gone = new AbortController();
  const on = listener(gone.signal, true);
  // The first non-zero motion once locked is the cursor jumping to where Chrome parks it, not
  // the user's: dropped. Headless Chromium sends it (-640,-360 on a 1280x720 page) before
  // `pointerlockchange`, so the flag is set when the lock is asked for.
  let parked = false;
  on(canvas, 'click', () => {
    if (locked()) return;
    parked = true;
    canvas.focus();
    // Chrome returns a promise; a refusal (no gesture, headless) leaves keyboard-only capture.
    Promise.resolve(canvas.requestPointerLock()).catch(() => {});
  });
  on(doc, 'pointerlockchange', () => {
    if (!locked()) leave();
  });
  const key = (down: boolean) => (e: KeyboardEvent) => {
    if (!captured()) return;
    if (e.code === 'Escape') {
      if (!locked()) leave(); // without Pointer Lock, Escape is the way out of capture
      return;
    }
    const vk = CODE_TO_VK[e.code];
    if (vk === undefined) return;
    e.preventDefault(); // the browser ignores this for its reserved shortcuts, which is fine
    s.key(vk, down, modifiers(e)); // drops repeats: Sunshine repeats held keys itself
  };
  on(win, 'keydown', key(true), { passive: false });
  on(win, 'keyup', key(false), { passive: false });
  on(doc, 'mousemove', (e: MouseEvent) => {
    if (!locked()) return;
    if (!parked) s.move(e.movementX, e.movementY);
    else if (e.movementX || e.movementY) parked = false;
  });
  const button = (down: boolean) => (e: MouseEvent) => {
    if (!locked() || e.button < 0 || e.button > 4) return;
    e.preventDefault();
    s.button(e.button + 1, down); // 0 left, 1 middle, 2 right, 3 back, 4 forward -> 1..5
  };
  on(canvas, 'mousedown', button(true), { passive: false });
  on(canvas, 'mouseup', button(false), { passive: false });
  on(
    canvas,
    'wheel',
    (e: WheelEvent) => {
      if (!locked()) return;
      e.preventDefault();
      s.wheel(wheelUnits(e.deltaY, e.deltaMode));
    },
    { passive: false },
  );
  on(canvas, 'contextmenu', (e: Event) => e.preventDefault(), { passive: false });
  on(win, 'blur', leave);
  on(win, 'pagehide', leave);
  on(doc, 'visibilitychange', () => {
    if (doc.visibilityState === 'hidden') leave();
  });
  const stop = () => {
    s.releaseAll();
    gone.abort();
  };
  closed.then(stop, stop); // a failed close too, without an unhandled rejection
}

export interface Input {
  stats: InputStats;
  /// Key-ups and button-ups for everything held, and every pad removed, sent before the page
  /// closes the transport.
  release(): void;
}

/// Opens the input stream on `transport`, captures input on `canvas` and polls gamepads;
/// `onpads` hears which pads are attached.
export async function startInput(transport: WebTransport, canvas: HTMLCanvasElement, onpads?: (p: PadInfo[]) => void): Promise<Input> {
  const stream = await transport.createBidirectionalStream();
  const w = stream.writable.getWriter();
  // Keys, buttons and pad button changes always go; motion, wheel and stick-only pad frames are
  // refused while the stream is backed up, so a pad cannot starve the keyboard.
  const write = (f: Uint8Array, droppable: boolean) => {
    if (droppable && (w.desiredSize ?? 1) <= 0) return false;
    w.write(f).catch(() => {}); // a closed stream ends the session, which the page reports
    return true;
  };
  const s = new InputSender(write);
  canvas.tabIndex = 0;
  attach(s, { canvas, doc: document, win: window }, transport.closed);
  const pads = new GamepadSender(write, () => navigator.getGamepads(), onpads);
  attachGamepads(pads, { doc: document, win: window, raf: requestAnimationFrame, caf: cancelAnimationFrame }, transport.closed);
  void (async () => {
    const split = new FrameSplitter();
    const r = stream.readable.getReader();
    for (;;) {
      const { value, done } = await r.read().catch(() => ({ value: undefined, done: true }));
      if (done || !value) return;
      for (const b of split.push(value)) onBridgeFrame(pads, b);
    }
  })();
  return {
    stats: s.stats,
    release: () => {
      s.releaseAll();
      pads.detachAll();
    },
  };
}
