// 8BitDo Ultimate 2 Wireless, front view, for the Settings "Controller" diagram (PadDiagram.svelte).
// 1 unit = 1 mm, origin = centre of the 147 x 103 mm body box (official size), y down.
// Positions and sizes were measured off the vector line art in the official manual
// (download.8bitdo.com/Manual/Controller/Ultimate/Ultimate-2-Wireless-Controller-EN.pdf), scaled so its
// body box is 147 mm wide; accurate to about 1 mm. The shapes are our own geometry, not 8BitDo artwork.
// research/8bitdo-ultimate-2-dimensions.md has the sources and what is unverified.

export const VIEWBOX = '-80 -60 160 120';
export const BODY_MM = { w: 147, h: 103 };

type Seg = ['L', number, number] | ['C', number, number, number, number, number, number];
const d = (start: [number, number], segs: Seg[]) => `M${start.join(' ')} ` + segs.map((s) => s.join(' ')).join(' ');
const flip = (segs: Seg[]) => segs.map((s) => s.map((v, i) => (i % 2 ? -(v as number) : v)) as Seg);
/** The same outline walked backwards, so a mirrored half joins the original into one closed path. */
function reverse(start: [number, number], segs: Seg[]): Seg[] {
  const ends = [start, ...segs.map((s) => s.slice(-2) as [number, number])];
  return segs.map((s, i) => (s[0] === 'L' ? ['L', ...ends[i]] : ['C', s[3], s[4], s[1], s[2], ...ends[i]]) as Seg).reverse();
}

// Left half of the body, top centre -> bumper bump -> shoulder -> outer grip -> grip tip -> inner grip -> bottom centre.
const TOP: [number, number] = [0, -49.6];
const BUMP: Seg[] = [
  ['C', -30, -51.4, -33, -51.6, -36.6, -51.6],
  ['C', -40.5, -51.6, -45, -50.8, -48, -49.6],
  ['C', -51.5, -48, -55.5, -45.8, -58.2, -43.6],
];
const LEFT: Seg[] = [
  ['L', -26.4, -49.4],
  ...BUMP,
  ['C', -61.5, -40.5, -67.5, -28, -69.4, -15.6], // shoulder down the side
  ['C', -71.2, -3.8, -73.5, 12, -73.5, 24], // widest point (73.5) about 28 mm above the grip tip
  ['C', -73.5, 43, -65.5, 50, -57.5, 51.6], // round under the outer grip
  ['C', -56.3, 51.7, -55.4, 51, -54.9, 50.1], // grip tip
  ['C', -50, 42.5, -40, 24.8, -34.1, 20.7], // inner grip edge
  ['C', -32.5, 19.6, -30.5, 19.2, -28.5, 19.2],
  ['L', 0, 19],
];
export const BODY =
  d(TOP, LEFT) +
  ' ' +
  reverse(TOP, flip(LEFT))
    .map((s) => s.join(' '))
    .join(' ') +
  ' Z';

// Bumper as seen from the front: the band between the top edge and the bumper's lower seam.
const LB: Seg[] = [...BUMP, ['C', -59.2, -42.8, -59.6, -41.6, -59.7, -40.4], ['C', -50, -44.6, -36, -47.2, -26.4, -49.4]];
const bumper = (segs: Seg[], sx: number) => d([-26.4 * sx, -49.4], segs) + ' Z';

export type Circle = { id: string; button?: number; label: string; x: number; y: number; r: number };
export type Shape = { id: string; button?: number; label: string; d: string; lx: number; ly: number };
export type Box = { id: string; button?: number; label: string; x: number; y: number; w: number; h: number };

// Standard-mapping buttons (Chrome's 17). Each id gets lit by its button index.
export const STICKS: (Circle & { ring: number; travel: number })[] = [
  { id: 'pad-l3', button: 10, label: 'L3', x: -42.5, y: -22.8, r: 7.4, ring: 12.15, travel: 4.75 },
  { id: 'pad-r3', button: 11, label: 'R3', x: 20.3, y: 0.8, r: 7.4, ring: 12.15, travel: 4.75 },
];
export const FACE: Circle[] = [
  { id: 'pad-a', button: 0, label: 'A', x: 42.4, y: -12.3, r: 4.85 },
  { id: 'pad-b', button: 1, label: 'B', x: 53, y: -22.9, r: 4.85 },
  { id: 'pad-x', button: 2, label: 'X', x: 31.9, y: -22.9, r: 4.85 },
  { id: 'pad-y', button: 3, label: 'Y', x: 42.5, y: -33.4, r: 4.85 },
];
export const MIDDLE: Circle[] = [
  { id: 'pad-back', button: 8, label: 'Back', x: -19.3, y: -35.4, r: 3.6 },
  { id: 'pad-start', button: 9, label: 'Start', x: 19.3, y: -35.4, r: 3.6 },
  { id: 'pad-home', button: 16, label: 'Home', x: 0, y: -35.3, r: 4.8 },
];
export const BUMPERS: Shape[] = [
  { id: 'pad-lb', button: 4, label: 'LB', d: bumper(LB, 1), lx: -38, ly: -48.4 },
  { id: 'pad-rb', button: 5, label: 'RB', d: bumper(flip(LB), -1), lx: 38, ly: -48.4 },
];
/** Triggers sit behind the bumpers: drawn as a top-down projection above the top edge, with an analog fill bar. */
export const TRIGGERS: Box[] = [
  { id: 'pad-lt', button: 6, label: 'LT', x: -60, y: -59.5, w: 20, h: 6 },
  { id: 'pad-rt', button: 7, label: 'RT', x: 40, y: -59.5, w: 20, h: 6 },
];

// D-pad: a cross 25.6 mm tip to tip, arms 9.6 mm wide, one path per direction so each lights on its own.
export const DPAD_C = { x: -20.3, y: 0.7, half: 12.8, arm: 4.8, dish: 3.2 };
const arm = (ux: number, uy: number) => {
  const { x, y, half: h, arm: a } = DPAD_C,
    r = 1.6; // rounded outer corners
  const P = (along: number, across: number) => `${(x + ux * along - uy * across).toFixed(2)} ${(y + uy * along + ux * across).toFixed(2)}`;
  return `M${P(a, -a)} L${P(h - r, -a)} Q${P(h, -a)} ${P(h, -a + r)} L${P(h, a - r)} Q${P(h, a)} ${P(h - r, a)} L${P(a, a)} Z`;
};
export const DPAD: Shape[] = [
  { id: 'pad-up', button: 12, label: '↑', d: arm(0, -1), lx: DPAD_C.x, ly: DPAD_C.y - 8.8 },
  { id: 'pad-down', button: 13, label: '↓', d: arm(0, 1), lx: DPAD_C.x, ly: DPAD_C.y + 8.8 },
  { id: 'pad-left', button: 14, label: '←', d: arm(-1, 0), lx: DPAD_C.x - 8.8, ly: DPAD_C.y },
  { id: 'pad-right', button: 15, label: '→', d: arm(1, 0), lx: DPAD_C.x + 8.8, ly: DPAD_C.y },
];

// Extra inputs the pad has but browsers never see (not in the standard mapping): drawn faint.
export const EXTRA_BOXES: Box[] = [
  { id: 'pad-l4', label: 'L4', x: -37, y: -58.5, w: 9, h: 5 },
  { id: 'pad-r4', label: 'R4', x: 28, y: -58.5, w: 9, h: 5 },
  { id: 'pad-profile', label: '', x: -3.5, y: -13.95, w: 7, h: 4.1 },
];
export const EXTRA_CIRCLES: Circle[] = [
  { id: 'pad-square', label: '', x: -10, y: -24.3, r: 3.6 },
  { id: 'pad-star', label: '', x: 10, y: -24.3, r: 3.6 },
];
// Back paddles, seen through the body (dashed): PL behind the left grip, PR behind the right.
const PL: Seg[] = [
  ['C', -26, 1, -24.6, 8.5, -27.5, 12],
  ['C', -30, 14.5, -36.5, 13.5, -39, 10.5],
  ['C', -41.5, 7, -38, 1, -32, 1],
];
export const PADDLES: Shape[] = [
  { id: 'pad-pl', label: 'PL', d: d([-32, 1], PL) + ' Z', lx: -32.3, ly: 7.5 },
  { id: 'pad-pr', label: 'PR', d: d([32, 1], flip(PL)) + ' Z', lx: 32.3, ly: 7.5 },
];
export const USBC: Box = { id: 'pad-usbc', label: '', x: -4.5, y: -52.4, w: 9, h: 3.2 };

/** Every drawn control, for the id checks. */
export const ALL = [...STICKS, ...FACE, ...MIDDLE, ...BUMPERS, ...TRIGGERS, ...DPAD, ...EXTRA_BOXES, ...EXTRA_CIRCLES, ...PADDLES, USBC];
