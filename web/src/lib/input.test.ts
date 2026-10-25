import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  attach, buttonFrame, CODE_TO_VK, FLUSH_MS, HEARTBEAT_MS, heartbeatFrame, InputSender, keyFrame, KIND_BUTTON, KIND_KEY, KIND_MOVE,
  KIND_SCROLL, modifiers, moveFrame, scrollFrame, wheelUnits,
} from './input';

const range = (p: string, n: number, from = 0) => Array.from({ length: n }, (_, i) => `${p}${i + from}`);
const letters = Array.from({ length: 26 }, (_, i) => `Key${String.fromCharCode(65 + i)}`);
const REQUIRED = [
  ...letters, ...range('Digit', 10), ...range('F', 12, 1), ...range('Numpad', 10),
  'ArrowLeft', 'ArrowUp', 'ArrowRight', 'ArrowDown',
  'ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight',
  'Enter', 'Escape', 'Backspace', 'Tab', 'Space', 'CapsLock', 'Insert', 'Delete', 'Home', 'End', 'PageUp', 'PageDown',
  'NumpadMultiply', 'NumpadAdd', 'NumpadSubtract', 'NumpadDecimal', 'NumpadDivide', 'NumpadEnter', 'NumLock',
  'Semicolon', 'Equal', 'Comma', 'Minus', 'Period', 'Slash', 'Backquote', 'BracketLeft', 'Backslash', 'BracketRight', 'Quote',
];

describe('CODE_TO_VK', () => {
  it('maps spot checks to Windows virtual-key codes', () => {
    expect(CODE_TO_VK.KeyA).toBe(0x41);
    expect(CODE_TO_VK.KeyZ).toBe(0x5a);
    expect(CODE_TO_VK.Digit0).toBe(0x30);
    expect(CODE_TO_VK.F1).toBe(0x70);
    expect(CODE_TO_VK.F12).toBe(0x7b);
    expect(CODE_TO_VK.ArrowLeft).toBe(0x25);
    expect(CODE_TO_VK.ArrowDown).toBe(0x28);
    expect(CODE_TO_VK.ShiftLeft).toBe(0xa0);
    expect(CODE_TO_VK.ControlRight).toBe(0xa3);
    expect(CODE_TO_VK.MetaLeft).toBe(0x5b);
    expect(CODE_TO_VK.Numpad7).toBe(0x67);
    expect(CODE_TO_VK.NumpadDecimal).toBe(0x6e);
    expect(CODE_TO_VK.Semicolon).toBe(0xba);
    expect(CODE_TO_VK.Quote).toBe(0xde);
    expect(CODE_TO_VK.Space).toBe(0x20);
  });
  it('misses none of the keys it should have', () => {
    expect(REQUIRED.filter((c) => !(c in CODE_TO_VK))).toEqual([]);
  });
  it('gives each code its own vk (NumpadEnter is VK_RETURN, as Moonlight sends it)', () => {
    const seen = new Map<number, string>();
    const dups: string[] = [];
    for (const [code, vk] of Object.entries(CODE_TO_VK)) {
      expect(vk, code).toBeGreaterThan(0);
      expect(vk, code).toBeLessThan(0xff);
      if (seen.has(vk)) dups.push(`${seen.get(vk)}=${code}`);
      else seen.set(vk, code);
    }
    expect(dups).toEqual(['Enter=NumpadEnter']);
  });
});

describe('frames', () => {
  it('are len + kind + little-endian fields', () => {
    expect([...keyFrame(0x41, true, 1)]).toEqual([4, KIND_KEY, 0x41, 1, 1]);
    expect([...moveFrame(-5, 258)]).toEqual([5, KIND_MOVE, 0xfb, 0xff, 0x02, 0x01]);
    expect([...buttonFrame(3, false)]).toEqual([3, KIND_BUTTON, 3, 0]);
    expect([...scrollFrame(-120)]).toEqual([3, KIND_SCROLL, 0x88, 0xff]);
    expect([...heartbeatFrame]).toEqual([1, 0x14]);
  });
  it('modifier bits are shift 1, ctrl 2, alt 4, meta 8', () => {
    expect(modifiers({ shiftKey: true, ctrlKey: false, altKey: true, metaKey: false })).toBe(5);
    expect(modifiers({ shiftKey: false, ctrlKey: true, altKey: false, metaKey: true })).toBe(10);
  });
  it('wheel: one 100 px notch down is -120, lines and pages scale', () => {
    expect(wheelUnits(100, 0)).toBe(-120);
    expect(wheelUnits(-3, 1)).toBe(120);
    expect(wheelUnits(1, 2)).toBe(-120);
  });
});

function sender() {
  const out: number[][] = [];
  const s = new InputSender((f) => (out.push([...f]), true));
  return { s, out };
}

describe('InputSender', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('coalesces mouse motion into one message per flush interval', () => {
    const { s, out } = sender();
    for (let i = 0; i < 8; i++) s.move(1, -1); // a 1 kHz mouse for 8 ms
    expect(out).toEqual([]);
    vi.advanceTimersByTime(FLUSH_MS);
    expect(out).toEqual([[...moveFrame(8, -8)]]);
    vi.advanceTimersByTime(100);
    expect(out.length).toBe(1); // nothing new, nothing sent
  });

  it('sends motion before a button or key, so order is kept', () => {
    const { s, out } = sender();
    s.move(3, 0);
    s.button(1, true);
    s.move(0, 2);
    s.key(0x42, true, 0);
    expect(out).toEqual([[...moveFrame(3, 0)], [...buttonFrame(1, true)], [...moveFrame(0, 2)], [...keyFrame(0x42, true, 0)]]);
  });

  it('splits motion beyond i16', () => {
    const { s, out } = sender();
    s.move(40000, 0);
    s.flush();
    expect(out).toEqual([[...moveFrame(32767, 0)], [...moveFrame(40000 - 32767, 0)]]);
  });

  it('drops motion the writer refuses (back-pressure) without losing later releases', () => {
    const out: number[][] = [];
    let open = false;
    const s = new InputSender((f, droppable) => (droppable && !open ? false : (out.push([...f]), true)));
    s.key(0x41, true, 0);
    s.move(5, 5);
    s.flush();
    expect(s.stats.dropped).toBe(1);
    s.key(0x41, false, 0);
    expect(out).toEqual([[...keyFrame(0x41, true, 0)], [...keyFrame(0x41, false, 0)]]);
    open = true;
  });

  it('sends a heartbeat only while something is held', () => {
    const { s, out } = sender();
    const beats = () => out.filter((f) => f.length === heartbeatFrame.length && f[1] === heartbeatFrame[1]).length;
    vi.advanceTimersByTime(HEARTBEAT_MS * 4);
    expect(beats()).toBe(0); // nothing held
    s.key(0x41, true, 0);
    vi.advanceTimersByTime(HEARTBEAT_MS * 4);
    expect(beats()).toBe(4);
    s.button(1, true);
    s.key(0x41, false, 0);
    vi.advanceTimersByTime(HEARTBEAT_MS * 2);
    expect(beats()).toBe(6); // the button is still down: one heartbeat, not two
    s.button(1, false);
    vi.advanceTimersByTime(HEARTBEAT_MS * 4);
    expect(beats()).toBe(6); // stops when the last thing is released
    s.key(0x42, true, 0);
    s.releaseAll();
    vi.advanceTimersByTime(HEARTBEAT_MS * 4);
    expect(beats()).toBe(6); // and on releaseAll
    expect(vi.getTimerCount()).toBe(0);
  });

  it('releaseAll lets go of every held key and button once', () => {
    const { s, out } = sender();
    s.key(0xa0, true, 1);
    s.key(0x41, true, 1);
    s.key(0x41, false, 1);
    s.button(3, true);
    out.length = 0;
    s.releaseAll();
    expect(out).toEqual([[...keyFrame(0xa0, false, 0)], [...buttonFrame(3, false)]]);
    s.releaseAll();
    expect(out.length).toBe(2);
  });
});

/// Plays the browser: the canvas, window and document as plain EventTargets.
function env() {
  const canvas = Object.assign(new EventTarget(), {
    requestPointerLock: vi.fn(),
    focus: vi.fn(() => (doc.activeElement = canvas)),
    blur: vi.fn(() => (doc.activeElement = null)),
  });
  const doc = Object.assign(new EventTarget(), {
    pointerLockElement: null as unknown,
    activeElement: null as unknown,
    visibilityState: 'visible',
  });
  const win = new EventTarget();
  const lock = () => {
    canvas.dispatchEvent(new Event('click')); // asks for the lock and focuses
    doc.pointerLockElement = canvas;
    doc.dispatchEvent(new Event('pointerlockchange'));
  };
  /// Headless Chromium's order: the lock is in place and the cursor jumps before
  /// `pointerlockchange` fires.
  const lockJumpFirst = () => {
    canvas.dispatchEvent(new Event('click'));
    doc.pointerLockElement = canvas;
    doc.dispatchEvent(Object.assign(new Event('mousemove'), { movementX: -640, movementY: -360 }));
    doc.dispatchEvent(new Event('pointerlockchange'));
  };
  return { canvas, doc, win, lock, lockJumpFirst };
}

const ev = (type: string, props: Record<string, unknown> = {}) => {
  const e = Object.assign(new Event(type, { cancelable: true }), { shiftKey: false, ctrlKey: false, altKey: false, metaKey: false }, props);
  return e;
};

describe('attach', () => {
  let close: (v?: unknown) => void;
  const setup = () => {
    const e = env();
    const { s, out } = sender();
    const closed = new Promise((r) => (close = r));
    attach(s, e, closed);
    return { ...e, s, out };
  };

  it('a click asks for pointer lock and is not sent', () => {
    const { canvas, out } = setup();
    canvas.dispatchEvent(ev('mousedown', { button: 0 }));
    canvas.dispatchEvent(ev('click', { button: 0 }));
    expect(canvas.requestPointerLock).toHaveBeenCalled();
    expect(out).toEqual([]);
  });

  it('sends keys by code while captured, with preventDefault, and skips repeats', () => {
    const { win, lock, out } = setup();
    const k = ev('keydown', { code: 'KeyA', key: 'q', repeat: false }); // AZERTY: code still KeyA
    win.dispatchEvent(k);
    expect(out).toEqual([]); // not captured yet
    lock();
    win.dispatchEvent(k);
    win.dispatchEvent(ev('keydown', { code: 'KeyA', repeat: true }));
    win.dispatchEvent(ev('keyup', { code: 'KeyA', shiftKey: true }));
    expect(out).toEqual([[...keyFrame(0x41, true, 0)], [...keyFrame(0x41, false, 1)]]);
    expect(k.defaultPrevented).toBe(true);
  });

  it('never sends Escape (the browser leaves pointer lock on it)', () => {
    const { win, lock, out } = setup();
    lock();
    win.dispatchEvent(ev('keydown', { code: 'Escape' }));
    win.dispatchEvent(ev('keyup', { code: 'Escape' }));
    expect(out).toEqual([]);
  });

  it('mouse motion, buttons and wheel only while locked', () => {
    const { canvas, doc, lock, out } = setup();
    canvas.focus();
    doc.dispatchEvent(ev('mousemove', { movementX: 9, movementY: 9 }));
    canvas.dispatchEvent(ev('mousedown', { button: 2 }));
    canvas.dispatchEvent(ev('wheel', { deltaY: 100, deltaMode: 0 }));
    vi.advanceTimersByTime(FLUSH_MS);
    expect(out).toEqual([]);
    lock();
    doc.dispatchEvent(ev('mousemove', { movementX: 0, movementY: 0 }));
    doc.dispatchEvent(ev('mousemove', { movementX: -640, movementY: -360 })); // the jump into the lock
    doc.dispatchEvent(ev('mousemove', { movementX: 5, movementY: -5 }));
    canvas.dispatchEvent(ev('mousedown', { button: 2 }));
    canvas.dispatchEvent(ev('mouseup', { button: 2 }));
    canvas.dispatchEvent(ev('wheel', { deltaY: 100, deltaMode: 0 }));
    vi.advanceTimersByTime(FLUSH_MS);
    expect(out).toEqual([[...moveFrame(5, -5)], [...buttonFrame(3, true)], [...buttonFrame(3, false)], [...scrollFrame(-120)]]);
  });

  it('drops the cursor jump into the lock even when it comes before pointerlockchange', () => {
    const { doc, lockJumpFirst, out } = setup();
    lockJumpFirst();
    doc.dispatchEvent(ev('mousemove', { movementX: 5, movementY: 0 }));
    vi.advanceTimersByTime(FLUSH_MS);
    expect(out).toEqual([[...moveFrame(5, 0)]]);
  });

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const held = (x: ReturnType<typeof setup>) => {
    x.lock();
    x.win.dispatchEvent(ev('keydown', { code: 'ShiftLeft', shiftKey: true }));
    x.canvas.dispatchEvent(ev('mousedown', { button: 0 }));
    x.out.length = 0;
  };
  const released = [[...keyFrame(0xa0, false, 0)], [...buttonFrame(1, false)]];

  for (const [name, fire] of [
    ['window blur', (x: ReturnType<typeof setup>) => x.win.dispatchEvent(ev('blur'))],
    ['visibilitychange to hidden', (x: ReturnType<typeof setup>) => {
      x.doc.visibilityState = 'hidden';
      x.doc.dispatchEvent(ev('visibilitychange'));
    }],
    ['pagehide', (x: ReturnType<typeof setup>) => x.win.dispatchEvent(ev('pagehide'))],
    ['pointer lock loss', (x: ReturnType<typeof setup>) => {
      x.doc.pointerLockElement = null;
      x.doc.dispatchEvent(ev('pointerlockchange'));
    }],
  ] as const) {
    it(`releases everything on ${name}`, () => {
      const x = setup();
      held(x);
      fire(x);
      expect(x.out).toEqual(released);
    });
  }

  it('pointer lock loss also ends keyboard capture', () => {
    const x = setup();
    held(x);
    x.doc.pointerLockElement = null;
    x.doc.dispatchEvent(ev('pointerlockchange'));
    x.out.length = 0;
    x.win.dispatchEvent(ev('keydown', { code: 'KeyB' }));
    expect(x.out).toEqual([]);
  });

  it('releases everything when the transport closes', async () => {
    const x = setup();
    held(x);
    close();
    await vi.runAllTimersAsync();
    expect(x.out).toEqual(released);
  });
});
