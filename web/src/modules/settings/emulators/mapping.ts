// Pure data and rules of the Emulator mappings editor (docs/plans/2026-10-28-emulator-controls-v2.md, sections 1.5 and 1.6).
import type { Bindings, ControllerType } from '../../../state/api';
import { PAD_LABEL, PAD_NAMES, type PadName } from '../pad.svg';

export type Kind = 'button' | 'stick' | 'dpad';
/** `name` is listed and spoken, `label` (when it differs) is drawn under the pad button. */
export type ControlDef = { id: string; name: string; kind: Kind; label?: string };

const btn = (id: string, name: string, label?: string): ControlDef => ({ id, name, kind: 'button', label });
export const CONTROL_SETS: Record<ControllerType, ControlDef[]> = {
  classic: [
    btn('a', 'A'),
    btn('b', 'B'),
    btn('x', 'X'),
    btn('y', 'Y'),
    btn('l', 'L'),
    btn('r', 'R'),
    btn('zl', 'ZL'),
    btn('zr', 'ZR'),
    btn('minus', 'Minus', '−'),
    btn('plus', 'Plus', '+'),
    btn('home', 'Home'),
    { id: 'ls', name: 'Left stick', kind: 'stick' },
    { id: 'rs', name: 'Right stick', kind: 'stick' },
    { id: 'dp', name: 'D-pad', kind: 'dpad' },
  ],
  gamecube: [
    btn('a', 'A'),
    btn('b', 'B'),
    btn('x', 'X'),
    btn('y', 'Y'),
    btn('z', 'Z'),
    btn('start', 'Start'),
    btn('l', 'L'),
    btn('r', 'R'),
    { id: 'ms', name: 'Control stick', kind: 'stick' },
    { id: 'cs', name: 'C stick', kind: 'stick' },
    { id: 'dp', name: 'D-pad', kind: 'dpad' },
  ],
};
export const TYPE_LABEL: Record<ControllerType, string> = { classic: 'Classic Controller', gamecube: 'GameCube controller' };
export const idsOf = (t: ControllerType) => CONTROL_SETS[t].map((c) => c.id);
export const controlOf = (t: ControllerType, id: string) => CONTROL_SETS[t].find((c) => c.id === id)!;

const BUTTON_NAMES: readonly PadName[] = ['A', 'B', 'X', 'Y', 'LB', 'RB', 'LT', 'RT', 'View', 'Menu', 'Guide'];
const STICK_NAMES: readonly PadName[] = ['Left stick', 'Right stick'];
/** Which pad names a control of this kind takes (the allowlist hostd checks too). */
export const ACCEPTS: Record<Kind, readonly string[]> = { button: BUTTON_NAMES, stick: STICK_NAMES, dpad: ['D-pad'] };
export const accepts = (t: ControllerType, id: string, pad: string) => ACCEPTS[controlOf(t, id).kind].includes(pad);

export const kindOf = (pad: string): Kind => (pad === 'D-pad' ? 'dpad' : pad.endsWith('stick') ? 'stick' : 'button');

/** The default mappings (the same as hostd's, and as the fixtures). */
export const DEFAULTS: Record<ControllerType, Bindings> = {
  classic: {
    a: 'B',
    b: 'A',
    x: 'Y',
    y: 'X',
    l: 'LT',
    r: 'RT',
    zl: 'LB',
    zr: 'RB',
    minus: 'View',
    plus: 'Menu',
    home: 'Guide',
    ls: 'Left stick',
    rs: 'Right stick',
    dp: 'D-pad',
  },
  gamecube: { a: 'A', b: 'X', x: 'B', y: 'Y', z: 'RB', start: 'Menu', l: 'LT', r: 'RT', ms: 'Left stick', cs: 'Right stick', dp: 'D-pad' },
};

/** Gamepad API standard-mapping button index -> pad name. 10 and 11 (stick clicks) map to nothing. */
export const PAD_BY_INDEX: Record<number, PadName> = {
  0: 'A',
  1: 'B',
  2: 'X',
  3: 'Y',
  4: 'LB',
  5: 'RB',
  6: 'LT',
  7: 'RT',
  8: 'View',
  9: 'Menu',
  12: 'D-pad',
  13: 'D-pad',
  14: 'D-pad',
  15: 'D-pad',
  16: 'Guide',
};

/** The control that holds this pad name, if any. */
export const padHolder = (b: Bindings, pad: string) => Object.keys(b).find((id) => b[id] === pad) ?? null;
/** Pad names no control holds. */
export const unbound = (b: Bindings): PadName[] => PAD_NAMES.filter((p) => padHolder(b, p) === null);
/** `pad name -> short text` for the drawing: what each pad button does. */
export const drawnLabels = (t: ControllerType, b: Bindings): Partial<Record<PadName, string>> =>
  Object.fromEntries(CONTROL_SETS[t].flatMap((c) => (b[c.id] ? [[b[c.id], c.label ?? c.name]] : [])));

export type Bound = { bindings: Bindings; swappedWith: string | null; refused: string | null };

/**
 * Bind `pad` to control `id`. A pad button drives one control, so the control that held it takes this one's old pad
 * button (swap). A custom control (null) has nothing to hand over: then a held name is refused. A name the control
 * does not accept, or the one it already has, changes nothing.
 */
export function bind(t: ControllerType, bindings: Bindings, id: string, pad: string): Bound {
  const same = { bindings, swappedWith: null, refused: null };
  if (!accepts(t, id, pad) || bindings[id] === pad) return same;
  const other = padHolder(bindings, pad);
  if (other && bindings[id] === null) {
    const on = controlOf(t, other).name;
    return { ...same, refused: `${PAD_LABEL[pad as PadName]} is used by ${on}. Change ${on} first.` };
  }
  const next = { ...bindings, [id]: pad };
  if (other) next[other] = bindings[id];
  return { bindings: next, swappedWith: other, refused: null };
}
