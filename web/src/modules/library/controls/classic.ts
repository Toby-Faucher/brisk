// Pure data and rules of the Classic Controller editor (docs/plans/2026-10-27-emulator-controls.md, sections 2 and 5).
import type { Bindings, ControlId } from '../../../state/api';

export type Kind = 'button' | 'stick' | 'dpad';
/** `label` is drawn on the control, `name` is spoken and written in messages. */
export const CONTROLS: { id: ControlId; label: string; name: string; kind: Kind }[] = [
  { id: 'a', label: 'A', name: 'A', kind: 'button' },
  { id: 'b', label: 'B', name: 'B', kind: 'button' },
  { id: 'x', label: 'X', name: 'X', kind: 'button' },
  { id: 'y', label: 'Y', name: 'Y', kind: 'button' },
  { id: 'l', label: 'L', name: 'L', kind: 'button' },
  { id: 'r', label: 'R', name: 'R', kind: 'button' },
  { id: 'zl', label: 'ZL', name: 'ZL', kind: 'button' },
  { id: 'zr', label: 'ZR', name: 'ZR', kind: 'button' },
  { id: 'minus', label: '−', name: 'Minus', kind: 'button' },
  { id: 'plus', label: '+', name: 'Plus', kind: 'button' },
  { id: 'home', label: 'Home', name: 'Home', kind: 'button' },
  { id: 'ls', label: 'Left', name: 'Left stick', kind: 'stick' },
  { id: 'rs', label: 'Right', name: 'Right stick', kind: 'stick' },
  { id: 'dp', label: 'D-pad', name: 'D-pad', kind: 'dpad' },
];
export const CONTROL_IDS = CONTROLS.map((c) => c.id);
export const control = (id: ControlId) => CONTROLS.find((c) => c.id === id)!;

export const BUTTON_NAMES = ['A', 'B', 'X', 'Y', 'LB', 'RB', 'LT', 'RT', 'View', 'Menu', 'Guide'] as const;
export const STICK_NAMES = ['Left stick', 'Right stick'] as const;
export const PAD_NAMES: readonly string[] = [...BUTTON_NAMES, ...STICK_NAMES, 'D-pad'];

/** Which pad names each control takes (the allowlist, the same table hostd checks). */
export const ACCEPTS: Record<ControlId, readonly string[]> = Object.fromEntries(
  CONTROLS.map((c) => [c.id, c.kind === 'button' ? BUTTON_NAMES : c.kind === 'stick' ? STICK_NAMES : ['D-pad']]),
) as Record<ControlId, readonly string[]>;

/** The known-good file's mapping (deploy/dolphin/WiimoteNew.ini), not the mock-up's. */
export const XBOX_DEFAULT: Bindings = {
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
};

/** Standard-mapping button index -> pad name. 10 and 11 (stick clicks) map to nothing. */
export const PAD_BY_INDEX: Record<number, string> = {
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

export const kindOf = (pad: string): Kind => (pad === 'D-pad' ? 'dpad' : pad.endsWith('stick') ? 'stick' : 'button');

export type Bound = { bindings: Bindings; swappedWith: ControlId | null };

/**
 * Bind `pad` to `id`. A pad name drives one control, so the control that held it takes this one's old binding.
 * When this control was custom (null) there is nothing to hand over: the other takes the first free name it accepts.
 * A name the control does not accept, or the one it already has, changes nothing.
 */
export function bind(bindings: Bindings, id: ControlId, pad: string): Bound {
  if (!ACCEPTS[id].includes(pad) || bindings[id] === pad) return { bindings, swappedWith: null };
  const next = { ...bindings, [id]: pad };
  const other = CONTROL_IDS.find((k) => k !== id && bindings[k] === pad) ?? null;
  if (other) next[other] = bindings[id] ?? ACCEPTS[other].find((n) => !Object.values(next).includes(n)) ?? null;
  return { bindings: next, swappedWith: other };
}
