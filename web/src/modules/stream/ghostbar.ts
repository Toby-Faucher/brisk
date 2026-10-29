// Pure logic of the Stream page's ghost bar: when the pill shows, the hold-Esc release, and the
// Advanced quality request.
import type { Codec, StreamReq } from '../../state/api';
export const EDGE_PX = 4;
export const HIDE_MS = 600;
const HOLD_MS = 800;

/** Is the bar visible? `sinceLeaveMs`: time since the pointer left the bar (caller's clock). */
export function edgeReveal(y: number, prevVisible: boolean, hovering: boolean, menuOpen = false, sinceLeaveMs = 0): boolean {
  if (y <= EDGE_PX || menuOpen) return true;
  return prevVisible && (hovering || sinceLeaveMs < HIDE_MS);
}

export type Hold = { progress: number; fired: boolean };

/** Esc held for HOLD_MS fires once; a tap does nothing and keydown auto-repeat never restarts it. */
export function holdEsc() {
  let start: number | null = null;
  const at = (t: number): Hold => {
    if (start === null) return { progress: 0, fired: false };
    const progress = Math.min(1, (t - start) / HOLD_MS);
    return { progress, fired: progress >= 1 };
  };
  return {
    press: (t: number) => ((start ??= t), at(t)),
    release: (_t: number) => ((start = null), at(0)),
    tick: at,
  };
}

export type Advanced = { res: string; fps: number; bitrateKbps: number; codec: Codec };

/** The Advanced panel's custom-quality request (`res` is `WxH`). */
export function advancedReq(a: Advanced): StreamReq {
  const [width, height] = a.res.split('x').map(Number);
  return { preset: 'custom', width, height, fps: a.fps, bitrateKbps: a.bitrateKbps, codec: a.codec };
}
