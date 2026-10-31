// Controller navigation for the app UI: d-pad or left stick moves focus, A clicks, B sends Escape.
// Off on /stream, where the pad belongs to the game (lib/gamepad.ts).
export type Dir = 'up' | 'down' | 'left' | 'right';
export type Box = { left: number; top: number; right: number; bottom: number };

const cx = (b: Box) => (b.left + b.right) / 2;
const cy = (b: Box) => (b.top + b.bottom) / 2;

/** Index of the nearest box in `dir` from `from`, or -1. Off-axis distance counts double so rows and columns hold. */
export function pick(from: Box, dir: Dir, boxes: readonly Box[]): number {
  const horiz = dir === 'left' || dir === 'right';
  const sign = dir === 'right' || dir === 'down' ? 1 : -1;
  let best = -1;
  let bestScore = Infinity;
  boxes.forEach((b, i) => {
    const along = (horiz ? cx(b) - cx(from) : cy(b) - cy(from)) * sign;
    if (along <= 1) return;
    const across = Math.abs(horiz ? cy(b) - cy(from) : cx(b) - cx(from));
    const score = along + across * 2;
    if (score < bestScore) (bestScore = score), (best = i);
  });
  return best;
}

const FOCUSABLE = 'a[href],button,input,select,textarea,summary,[tabindex]:not([tabindex="-1"])';
const STICK = 0.6;
const FIRST_MS = 400;
const REPEAT_MS = 130;

function dirOf(g: Gamepad): Dir | null {
  const b = (i: number) => !!g.buttons[i]?.pressed;
  const [x = 0, y = 0] = g.axes;
  if (b(12) || y < -STICK) return 'up';
  if (b(13) || y > STICK) return 'down';
  if (b(14) || x < -STICK) return 'left';
  if (b(15) || x > STICK) return 'right';
  return null;
}

function move(dir: Dir) {
  const root = document.querySelector('dialog[open], [aria-modal="true"]') ?? document;
  const els = [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (e) => !(e as HTMLButtonElement).disabled && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== 'hidden',
  );
  if (!els.length) return;
  const cur = document.activeElement as HTMLElement | null;
  const at = cur && els.includes(cur) ? cur : null;
  if (!at) return els[0].focus();
  const others = els.filter((e) => e !== at);
  const i = pick(
    at.getBoundingClientRect(),
    dir,
    others.map((e) => e.getBoundingClientRect()),
  );
  if (i >= 0) {
    others[i].focus();
    others[i].scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }
}

/** Starts the poll loop; returns the disposer. */
export function attachPadNav(): () => void {
  let raf = 0;
  let held: Dir | null = null;
  let nextAt = 0;
  let aDown = false;
  let bDown = false;
  const root = document.documentElement;
  const mouse = () => root.removeAttribute('data-pad');
  addEventListener('pointerdown', mouse);
  const tick = (now: number) => {
    raf = requestAnimationFrame(tick);
    if (!document.hasFocus()) return; // the Gamepad API reports nothing useful for a background page
    const g = [...(navigator.getGamepads?.() ?? [])].find((p) => p?.connected);
    if (!g) return;
    const d = dirOf(g);
    if (d !== held) {
      held = d;
      nextAt = now + FIRST_MS;
      if (d) root.setAttribute('data-pad', ''), move(d);
    } else if (d && now >= nextAt) {
      nextAt = now + REPEAT_MS;
      move(d);
    }
    const a = !!g.buttons[0]?.pressed;
    if (a && !aDown) root.setAttribute('data-pad', ''), (document.activeElement as HTMLElement | null)?.click();
    aDown = a;
    const b = !!g.buttons[1]?.pressed;
    if (b && !bDown)
      (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    bDown = b;
  };
  raf = requestAnimationFrame(tick);
  return () => (cancelAnimationFrame(raf), removeEventListener('pointerdown', mouse));
}
