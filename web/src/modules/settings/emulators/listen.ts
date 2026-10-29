// Which pad name was pressed, from successive PadView frames (docs/plans/2026-10-28-emulator-controls-v2.md, section 1.5).
import type { PadView } from '../controllertest';
import { kindOf, PAD_BY_INDEX, type ControlDef, type Kind } from './mapping';

const TRIGGER_ON = 0.5;
const STICK_ON = 0.6;
const STICK_OFF = 0.3;

export type Heard = { type: 'none' } | { type: 'bind'; pad: string } | { type: 'wrong'; pad: string; hint: string } | { type: 'cancel' };

const NONE: Heard = { type: 'none' };
const WORD: Record<Kind, string> = { button: 'a button', stick: 'a stick', dpad: 'the D-pad' };

/** "That is a stick. A takes a button." */
export const wrongKind = (c: Pick<ControlDef, 'name' | 'kind'>, pad: string) =>
  `That is ${WORD[kindOf(pad)]}. ${c.name} takes ${WORD[c.kind]}.`;

/**
 * One listen for one control. Feed it a frame per tick (`null`: no pad). The first frame is the baseline: whatever is
 * held then is ignored until it was released once. The first new press (lowest button index, then left stick, then
 * right) answers; a press of the wrong kind answers `wrong` and listening goes on. A pad that was there and is gone
 * cancels; with no pad yet it just waits.
 */
export class Listener {
  private prev: Set<string> | null = null;
  private ignore = new Set<string>();
  private stick = { Left: false, Right: false };
  constructor(readonly control: Pick<ControlDef, 'name' | 'kind'> & { id: string }) {}

  private down(v: PadView): string[] {
    const keys: string[] = [];
    for (const b of v.buttons) {
      const on = b.index === 6 || b.index === 7 ? b.value > TRIGGER_ON : b.pressed;
      if (on && PAD_BY_INDEX[b.index]) keys.push(`b${b.index}`);
    }
    for (const [k, s] of [
      ['Left', v.left],
      ['Right', v.right],
    ] as const) {
      this.stick[k] = s.mag > (this.stick[k] ? STICK_OFF : STICK_ON); // pushed past 0.6, held until back under 0.3
      if (this.stick[k]) keys.push(`s${k}`);
    }
    return keys;
  }

  push(v: PadView | null): Heard {
    if (!v) return this.prev ? { type: 'cancel' } : NONE;
    const now = this.down(v);
    const first = !this.prev;
    const prev = this.prev ?? new Set<string>();
    this.prev = new Set(now);
    if (first) {
      this.ignore = new Set(now);
      return NONE;
    }
    for (const k of this.ignore) if (!now.includes(k)) this.ignore.delete(k);
    const fresh = now.find((k) => !prev.has(k) && !this.ignore.has(k));
    if (!fresh) return NONE;
    const pad = fresh[0] === 's' ? `${fresh.slice(1)} stick` : PAD_BY_INDEX[Number(fresh.slice(1))];
    return this.control.kind === kindOf(pad) ? { type: 'bind', pad } : { type: 'wrong', pad, hint: wrongKind(this.control, pad) };
  }
}
