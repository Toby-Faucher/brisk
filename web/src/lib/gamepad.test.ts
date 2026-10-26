import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ACTIVE_RESEND_MS, STICK_STEP, TRIGGER_STEP, attachGamepads, BUTTON_FLAGS, FrameSplitter, GamepadSender, goneFrame, IDLE_RESEND_MS, KIND_GAMEPAD,
  KIND_GAMEPAD_GONE, KIND_RUMBLE, MAX_PADS, onBridgeFrame, padFrame, padName, POLL_MS, readPad, RUMBLE_MS, RUMBLE_REFRESH_MS, type PadState,
} from './gamepad';

const N: PadState = { buttons: 0, lt: 0, rt: 0, lx: 0, ly: 0, rx: 0, ry: 0 };

type Fake = ReturnType<typeof fakePad>;
function fakePad(index: number, id = '8BitDo Ultimate 2 Wireless (STANDARD GAMEPAD Vendor: 2dc8 Product: 6012)') {
  return {
    index,
    id,
    connected: true,
    mapping: 'standard',
    buttons: Array.from({ length: 17 }, () => ({ pressed: false, value: 0 })),
    axes: [0, 0, 0, 0],
    vibrationActuator: { effects: ['dual-rumble'], playEffect: vi.fn(async () => 'complete'), reset: vi.fn(async () => 'complete') },
    press(i: number, v = 1) {
      this.buttons[i] = { pressed: v > 0, value: v };
    },
  };
}

/** Decodes what the sender wrote: [kind, slot, state?]. */
function decode(f: Uint8Array): [number, number, PadState?] {
  expect(f[0]).toBe(f.length - 1);
  if (f[1] === KIND_GAMEPAD_GONE) return [f[1], f[2]];
  const v = new DataView(f.buffer, f.byteOffset);
  return [f[1], f[2], { buttons: v.getUint16(3, true), lt: f[5], rt: f[6], lx: v.getInt16(7, true), ly: v.getInt16(9, true), rx: v.getInt16(11, true), ry: v.getInt16(13, true) }];
}

function rig(pads: (Fake | null)[] = [fakePad(0)]) {
  const sent: [number, number, PadState?][] = [];
  const list = { pads };
  const onpads = vi.fn();
  // `full`: the stream is backed up, droppable frames are refused (and not recorded).
  const w = { full: false, refused: 0 };
  const s = new GamepadSender(
    (f, droppable) => {
      if (droppable && w.full) return void w.refused++, false;
      sent.push(decode(f));
      return true;
    },
    () => list.pads,
    onpads,
  );
  return { s, sent, list, onpads, w, take: () => sent.splice(0) };
}

describe('mapping', () => {
  it('maps every standard button to its Moonlight flag', () => {
    const want: Record<number, number> = {
      0: 0x1000, 1: 0x2000, 2: 0x4000, 3: 0x8000, 4: 0x0100, 5: 0x0200, 8: 0x0020, 9: 0x0010,
      10: 0x0040, 11: 0x0080, 12: 0x0001, 13: 0x0002, 14: 0x0004, 15: 0x0008, 16: 0x0400,
    };
    for (let i = 0; i < 17; i++) {
      const p = fakePad(0);
      p.press(i);
      expect(readPad(p).buttons, `button ${i}`).toBe(want[i] ?? 0);
    }
    expect(BUTTON_FLAGS[6]).toBe(0); // triggers are analog only
    expect(BUTTON_FLAGS[7]).toBe(0);
    const all = fakePad(0);
    for (let i = 0; i < 17; i++) all.press(i);
    expect(readPad(all).buttons).toBe(0xf7ff);
  });

  it('scales triggers 0..1 to 0..255', () => {
    const p = fakePad(0);
    p.press(6, 0.5);
    p.press(7, 1);
    expect([readPad(p).lt, readPad(p).rt]).toEqual([128, 255]);
    p.press(6, 1.2); // out of range clamps
    p.press(7, 0.001);
    expect([readPad(p).lt, readPad(p).rt]).toEqual([255, 0]);
  });

  it('maps sticks to i16 with Y inverted (up is positive)', () => {
    const p = fakePad(0);
    p.axes = [-1, -1, 1, 1]; // left stick up-left, right stick down-right
    expect(readPad(p)).toMatchObject({ lx: -32768, ly: 32767, rx: 32767, ry: -32768 });
    p.axes = [0.5, 0.25, -0.5, 0];
    expect(readPad(p)).toMatchObject({ lx: 16384, ly: -8192, rx: -16384, ry: 0 });
    p.axes = [2, NaN, -3, 0];
    expect(readPad(p)).toMatchObject({ lx: 32767, ly: 0, rx: -32768 });
  });

  it('frames are len, kind, slot, little-endian state', () => {
    expect([...padFrame(1, { buttons: 0x1001, lt: 2, rt: 3, lx: -2, ly: 258, rx: 0, ry: 1 })]).toEqual([
      14, KIND_GAMEPAD, 1, 0x01, 0x10, 2, 3, 0xfe, 0xff, 0x02, 0x01, 0, 0, 1, 0,
    ]);
    expect([...goneFrame(3)]).toEqual([2, KIND_GAMEPAD_GONE, 3]);
  });

  it('names a pad without the vendor tail', () => {
    expect(padName('8BitDo Ultimate 2 Wireless (STANDARD GAMEPAD Vendor: 2dc8 Product: 6012)')).toBe('8BitDo Ultimate 2 Wireless');
    expect(padName('STANDARD GAMEPAD Vendor: 045e Product: 028e')).toBe('STANDARD GAMEPAD');
    expect(padName('')).toBe('Controller');
  });
});

describe('GamepadSender', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('sends on change only, plus a resend every 100 ms when active and 250 ms idle', () => {
    const { s, list, take } = rig();
    const p = list.pads[0]!;
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N]]); // attach
    s.poll();
    expect(take()).toEqual([]);
    p.press(0);
    s.poll();
    const A = { ...N, buttons: 0x1000 };
    expect(take()).toEqual([[KIND_GAMEPAD, 0, A]]);
    vi.advanceTimersByTime(ACTIVE_RESEND_MS - 1);
    s.poll();
    expect(take()).toEqual([]);
    vi.advanceTimersByTime(1);
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, A]]); // redundant resend
    p.press(0, 0);
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N]]);
    vi.advanceTimersByTime(ACTIVE_RESEND_MS);
    s.poll();
    expect(take()).toEqual([]); // neutral: slower keepalive
    vi.advanceTimersByTime(IDLE_RESEND_MS - ACTIVE_RESEND_MS);
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N]]);
  });

  it('a stick move is a change', () => {
    const { s, list, take } = rig();
    s.poll();
    take();
    list.pads[0]!.axes = [0, -1, 0, 0];
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, { ...N, ly: 32767 }]]);
  });

  it('gives pads the lowest free slot, at most 4, and frees it on removal', () => {
    const pads = [fakePad(0), null, fakePad(2), fakePad(3), fakePad(4), fakePad(5)];
    const { s, list, take, onpads } = rig(pads);
    s.poll();
    expect(take().map((f) => f[1])).toEqual([0, 1, 2, 3]);
    expect(onpads).toHaveBeenLastCalledWith([
      { slot: 0, name: '8BitDo Ultimate 2 Wireless' }, { slot: 1, name: '8BitDo Ultimate 2 Wireless' },
      { slot: 2, name: '8BitDo Ultimate 2 Wireless' }, { slot: 3, name: '8BitDo Ultimate 2 Wireless' },
    ]);
    list.pads = [fakePad(0), null, null, fakePad(3), fakePad(4), fakePad(5)];
    list.pads[0]!.press(0);
    s.poll();
    // Gamepad 2 (slot 1) went: it was neutral, so only the removal; then gamepad 4 takes slot 1.
    expect(take()).toEqual([[KIND_GAMEPAD_GONE, 1], [KIND_GAMEPAD, 0, { ...N, buttons: 0x1000 }], [KIND_GAMEPAD, 1, N]]);
    expect(MAX_PADS).toBe(4);
  });

  it('a resting stick drifting under the step waits for the resend; values are sent untouched', () => {
    const { s, list, take } = rig();
    const p = list.pads[0]!;
    p.axes = [0.5, 0, 0, 0];
    s.poll();
    const at = readPad(p);
    take();
    p.axes = [0.5 + (STICK_STEP - 1) / 32767, 0, 0, 0]; // just under a step
    p.press(7, (TRIGGER_STEP - 1) / 255);
    s.poll();
    expect(take()).toEqual([]);
    p.axes = [0.5 + STICK_STEP / 32767, 0, 0, 0];
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, readPad(p)]]);
    expect(readPad(p).lx - at.lx).toBe(STICK_STEP);
    p.press(7, (TRIGGER_STEP - 1 + TRIGGER_STEP) / 255); // a step from the 1 already sent
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, readPad(p)]]);
    p.axes = [0.5 + (STICK_STEP + 3) / 32767, 0, 0, 0]; // drift: sent with the next resend
    s.poll();
    expect(take()).toEqual([]);
    vi.advanceTimersByTime(ACTIVE_RESEND_MS);
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, readPad(p)]]);
    // A button always goes at once, and so does a return to exactly neutral.
    p.press(2);
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, readPad(p)]]);
    p.press(2, 0);
    p.press(7, 0);
    p.axes = [0, 0, 0, 0];
    s.poll();
    take();
    p.axes = [10 / 32767, 0, 0, 0];
    s.poll();
    expect(take()).toEqual([], 'small drift off neutral waits');
  });

  it('when the stream is backed up stick-only frames wait, button changes and removals do not', () => {
    const { s, list, w, take } = rig();
    const p = list.pads[0]!;
    s.poll();
    take();
    w.full = true;
    p.axes = [1, 0, 0, 0];
    s.poll();
    s.poll();
    expect(take()).toEqual([]);
    expect(w.refused).toBe(2); // tried each poll, not marked sent
    p.press(0);
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, readPad(p)]], 'a button change is never skipped');
    p.axes = [-1, 0, 0, 0];
    s.poll();
    expect(take()).toEqual([]);
    w.full = false;
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, readPad(p)]], 'the newest state once there is room');
    w.full = true;
    list.pads = [];
    s.poll();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N], [KIND_GAMEPAD_GONE, 0]], 'release and removal go anyway');
  });

  it('rumble that came before the pad is played when it appears; none while unfocused', () => {
    const p = fakePad(0);
    const { s, list } = rig([]);
    s.rumble(0, 0xffff, 0, 0, 0);
    list.pads = [p];
    s.poll();
    expect(p.vibrationActuator.playEffect).toHaveBeenCalledTimes(1);
    s.release(); // focus lost
    s.rumble(0, 0x8000, 0, 0, 0);
    expect(p.vibrationActuator.playEffect).toHaveBeenCalledTimes(1);
    s.poll(); // focus back: the newest values play
    expect(p.vibrationActuator.playEffect).toHaveBeenLastCalledWith('dual-rumble', expect.objectContaining({ strongMagnitude: 0x8000 / 0xffff }));
  });

  it('skips pads without the standard mapping', () => {
    const p = fakePad(0);
    p.mapping = '';
    const { s, take } = rig([p]);
    s.poll();
    expect(take()).toEqual([]);
  });

  it('disconnect sends neutral then removal and stops its rumble', () => {
    const p = fakePad(0);
    const { s, list, take, onpads } = rig([p]);
    p.press(1);
    s.poll();
    take();
    s.rumble(0, 0xffff, 0, 0, 0);
    s.disconnected(0);
    list.pads = [];
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N], [KIND_GAMEPAD_GONE, 0]]);
    expect(p.vibrationActuator.reset).toHaveBeenCalled();
    expect(onpads).toHaveBeenLastCalledWith([]);
    s.poll();
    expect(take()).toEqual([]);
  });

  it('release sends neutral for held pads but keeps them attached', () => {
    const p = fakePad(0);
    const { s, take } = rig([p]);
    p.press(5);
    p.axes = [0.5, 0, 0, 0];
    s.poll();
    take();
    s.release();
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N]]);
    s.release();
    expect(take()).toEqual([], 'once');
    s.poll(); // focus back, still held: sent again
    expect(take()).toEqual([[KIND_GAMEPAD, 0, readPad(p)]]);
    // A rumble is stopped once on release (release runs every tick while unfocused), then
    // replayed by the next focused poll.
    s.rumble(0, 0x4000, 0, 0, 0);
    s.release();
    s.release();
    expect(p.vibrationActuator.reset).toHaveBeenCalledTimes(1);
    s.poll();
    expect(p.vibrationActuator.playEffect).toHaveBeenCalledTimes(2);
  });

  it('detachAll removes every pad', () => {
    const { s, take } = rig([fakePad(0), fakePad(1)]);
    s.poll();
    take();
    s.detachAll();
    expect(take()).toEqual([[KIND_GAMEPAD_GONE, 0], [KIND_GAMEPAD_GONE, 1]]);
  });

  it('rumble plays dual-rumble with motors scaled to 0..1, refreshes, and stops on 0', () => {
    const p = fakePad(0);
    const { s } = rig([p]);
    s.poll();
    const play = p.vibrationActuator.playEffect;
    s.rumble(0, 0xffff, 0x8000, 0, 0);
    expect(play).toHaveBeenCalledTimes(1);
    expect(play).toHaveBeenLastCalledWith('dual-rumble', { duration: RUMBLE_MS, startDelay: 0, strongMagnitude: 1, weakMagnitude: 0x8000 / 0xffff });
    vi.advanceTimersByTime(RUMBLE_REFRESH_MS - 1);
    s.poll();
    expect(play).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1);
    s.poll();
    expect(play).toHaveBeenCalledTimes(2); // a long rumble outlasts one effect: refreshed
    s.rumble(0, 0, 0, 0, 0);
    expect(p.vibrationActuator.reset).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(RUMBLE_REFRESH_MS * 4);
    s.poll();
    expect(play).toHaveBeenCalledTimes(2);
    s.rumble(3, 1, 1, 0, 0); // no pad in slot 3: nothing
  });

  it('trigger rumble uses trigger-rumble only where the actuator has it', () => {
    const p = fakePad(0);
    p.vibrationActuator.effects = ['dual-rumble', 'trigger-rumble'];
    const { s } = rig([p]);
    s.poll();
    s.rumble(0, 0, 0, 0xffff, 0);
    expect(p.vibrationActuator.playEffect).toHaveBeenLastCalledWith('trigger-rumble', {
      duration: RUMBLE_MS, startDelay: 0, strongMagnitude: 0, weakMagnitude: 0, leftTrigger: 1, rightTrigger: 0,
    });
    const q = fakePad(0);
    const r = rig([q]);
    r.s.poll();
    r.s.rumble(0, 0, 0, 0xffff, 0xffff); // triggers only, no trigger motors here: nothing to play
    expect(q.vibrationActuator.playEffect).not.toHaveBeenCalled();
  });
});

describe('bridge frames', () => {
  it('a rumble frame drives the pad in its slot; other kinds and lengths are ignored', () => {
    const p = fakePad(0);
    const { s } = rig([p]);
    s.poll();
    onBridgeFrame(s, Uint8Array.of(KIND_RUMBLE, 0, 0xff, 0xff, 0, 0, 0, 0, 0, 0));
    expect(p.vibrationActuator.playEffect).toHaveBeenLastCalledWith('dual-rumble', expect.objectContaining({ strongMagnitude: 1, weakMagnitude: 0 }));
    onBridgeFrame(s, Uint8Array.of(KIND_RUMBLE, 0, 1));
    onBridgeFrame(s, Uint8Array.of(0x21, 0, 0xff, 0xff, 0, 0, 0, 0, 0, 0));
    expect(p.vibrationActuator.playEffect).toHaveBeenCalledTimes(1);
  });
});

describe('FrameSplitter', () => {
  it('reassembles frames across chunks', () => {
    const f = new FrameSplitter();
    const a = Uint8Array.of(10, KIND_RUMBLE, 1, 2, 3, 4, 5, 6, 7, 8, 9, 2, 0x99);
    expect(f.push(a.subarray(0, 4))).toEqual([]);
    const got = f.push(a.subarray(4));
    expect(got.map((x) => [...x])).toEqual([[KIND_RUMBLE, 1, 2, 3, 4, 5, 6, 7, 8, 9]]);
    expect(f.push(Uint8Array.of(0x98)).map((x) => [...x])).toEqual([[0x99, 0x98]]);
  });
});

describe('attachGamepads', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function env() {
    const doc = Object.assign(new EventTarget(), { focused: true, visibilityState: 'visible', hasFocus() { return this.focused; } });
    const win = new EventTarget();
    return { doc, win, raf: (f: () => void) => setTimeout(f, 16) as unknown as number, caf: (h: number) => clearTimeout(h) };
  }

  it('polls every 4 ms only while the document is focused and visible', () => {
    const { s, list, take } = rig();
    const e = env();
    let close!: () => void;
    attachGamepads(s, e, new Promise<void>((r) => (close = r)));
    vi.advanceTimersByTime(POLL_MS);
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N]]);
    list.pads[0]!.press(0);
    vi.advanceTimersByTime(POLL_MS);
    expect(take()).toEqual([[KIND_GAMEPAD, 0, { ...N, buttons: 0x1000 }]]);
    e.doc.focused = false;
    e.win.dispatchEvent(new Event('blur'));
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N]], 'released at once');
    vi.advanceTimersByTime(1000);
    expect(take()).toEqual([], 'no state while unfocused: the Gamepad API is stale there');
    e.doc.focused = true;
    vi.advanceTimersByTime(POLL_MS);
    expect(take()).toEqual([[KIND_GAMEPAD, 0, { ...N, buttons: 0x1000 }]]);
    e.doc.visibilityState = 'hidden';
    e.doc.dispatchEvent(new Event('visibilitychange'));
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N]]);
    close();
  });

  it('a transport that closes with an error still stops, without an unhandled rejection', async () => {
    const { s, take } = rig();
    const e = env();
    let fail!: (e: Error) => void;
    attachGamepads(s, e, new Promise<void>((_, r) => (fail = r)));
    vi.advanceTimersByTime(POLL_MS);
    take();
    fail(new Error('connection lost'));
    await vi.advanceTimersByTimeAsync(0);
    expect(take()).toEqual([[KIND_GAMEPAD_GONE, 0]]);
    vi.advanceTimersByTime(100);
    expect(take()).toEqual([]);
  });

  it('pagehide, a disconnect event and the transport closing detach pads', async () => {
    const { s, list, take } = rig([fakePad(0), fakePad(1)]);
    const e = env();
    let close!: () => void;
    attachGamepads(s, e, new Promise<void>((r) => (close = r)));
    vi.advanceTimersByTime(POLL_MS);
    take();
    list.pads = [list.pads[0]];
    e.win.dispatchEvent(Object.assign(new Event('gamepaddisconnected'), { gamepad: { index: 1 } }));
    expect(take()).toEqual([[KIND_GAMEPAD_GONE, 1]]);
    e.win.dispatchEvent(new Event('pagehide'));
    expect(take()).toEqual([[KIND_GAMEPAD_GONE, 0]]);
    vi.advanceTimersByTime(POLL_MS); // back from the bfcache: attached again
    expect(take()).toEqual([[KIND_GAMEPAD, 0, N]]);
    close();
    await vi.advanceTimersByTimeAsync(0);
    expect(take()).toEqual([[KIND_GAMEPAD_GONE, 0]]);
    vi.advanceTimersByTime(1000);
    expect(take()).toEqual([], 'stopped polling');
  });
});
