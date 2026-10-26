// Home card drag-to-reorder: an attachment on the bento grid. Cards carry `data-card` (their id) and a `[data-grip]`
// handle (ui/Card.svelte inside a home/Slot.svelte). Native Web Animations on `translate` only, no library: the lifted
// card follows the pointer, the others FLIP out of its way on the snappy spring, the drop settles on the settle spring.
// The lift itself (scale, shadow) is CSS on `[data-lifted]`. Rules: docs/ui-notes.md, Motion and Home reorder.
import { box, cssVar, motionAllowed, ms } from '../../ui/motion-base';
import { moveItem } from './home';

export type ReorderOpts = {
  /** The ids in their current order. */
  order: () => string[];
  /** Put the cards in this order, synchronously (state + flushSync): FLIP measures right after. */
  set: (ids: string[]) => void;
  /** A drop (not a cancel): where the card ended up and whether the order changed since the pick-up. */
  drop: (id: string, ids: string[], changed: boolean) => void;
};

type Drag = {
  id: string;
  el: HTMLElement;
  grip: HTMLElement;
  start: string[];
  pointer: number | null; // null: picked up with the keyboard
  gx: number; // where the card was grabbed, from its top left
  gy: number;
  tx: number; // the pointer-follow translate now
  ty: number;
  follow: Animation | null;
  last: string | null; // the card last swapped with: no swap back until the pointer leaves it (sizes differ)
};

const STEP: Record<string, number> = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 };

/** From (dx, dy) back to where the element is laid out; null for no move, reduced motion or /stream. */
function slide(el: Element, dx: number, dy: number, spring: string): Animation | null {
  if ((Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) || !motionAllowed()) return null;
  return el.animate([{ translate: `${dx}px ${dy}px` }, { translate: '0 0' }], { duration: ms(`${spring}-dur`, 600), easing: cssVar(spring) || 'ease-out' });
}

export function reorder(o: ReorderOpts) {
  return (bento: HTMLElement) => {
    const flights = new Map<Element, Animation>();
    let drag: Drag | null = null;
    let moving = false; // our own DOM moves can blur the handle: not a drop
    const cards = () => [...bento.querySelectorAll<HTMLElement>('[data-card]')];
    // cards are laid out relative to the bento (position: relative), offsets ignore every translate in flight
    const local = (e: PointerEvent) => {
      const r = bento.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    const inside = (c: HTMLElement, p: { x: number; y: number }) => p.x >= c.offsetLeft && p.x < c.offsetLeft + c.offsetWidth && p.y >= c.offsetTop && p.y < c.offsetTop + c.offsetHeight;
    const fly = (el: Element, a: Animation | null) => {
      if (!a) return;
      flights.set(el, a);
      a.onfinish = () => flights.get(el) === a && flights.delete(el);
    };

    /** Reorder and FLIP every card but `skip` from where it is drawn now to its new slot. */
    function shift(ids: string[], skip?: Element) {
      const els = cards().filter((e) => e !== skip);
      const from = new Map(els.map((e) => [e, box(e)]));
      for (const e of els) flights.get(e)?.cancel();
      bento.dataset.sorted = ''; // ends the load entrance for good (Home.svelte): a moved node would replay it
      moving = true;
      o.set(ids);
      moving = false;
      for (const e of els) {
        const a = from.get(e)!, b = box(e);
        fly(e, slide(e, a.x - b.x, a.y - b.y, '--spring-snappy'));
      }
    }

    function follow(d: Drag) {
      const t = `${d.tx}px ${d.ty}px`;
      // a zero-length animation held at its end: writes `translate` above the CSS reveal animation, no inline style
      if (d.follow) (d.follow.effect as KeyframeEffect).setKeyframes([{ translate: t }]);
      else d.follow = d.el.animate([{ translate: t }], { duration: 0, fill: 'forwards' });
    }

    function lift(grip: HTMLElement, pointer: number | null): Drag | null {
      const el = grip.closest<HTMLElement>('[data-card]');
      if (!el?.dataset.card) return null;
      flights.get(el)?.cancel();
      el.dataset.lifted = '';
      grip.setAttribute('aria-pressed', 'true');
      return (drag = { id: el.dataset.card, el, grip, start: o.order(), pointer, gx: 0, gy: 0, tx: 0, ty: 0, follow: null, last: null });
    }

    /** Drop (keep) or cancel (back to the order at pick-up). Leaves no attribute, style or animation behind but the landing. */
    function finish(keep: boolean) {
      const d = drag;
      if (!d) return;
      drag = null;
      removeEventListener('keydown', esc, true);
      if (d.pointer != null && bento.hasPointerCapture(d.pointer)) bento.releasePointerCapture(d.pointer);
      const x0 = d.el.offsetLeft + d.tx, y0 = d.el.offsetTop + d.ty; // where it is drawn, in bento coordinates
      if (!keep && o.order().join() !== d.start.join()) shift(d.start, d.pointer != null ? d.el : undefined);
      d.follow?.cancel();
      delete d.el.dataset.lifted; // the scale settles back by the CSS transition
      d.grip.setAttribute('aria-pressed', 'false');
      const land = d.pointer != null && slide(d.el, x0 - d.el.offsetLeft, y0 - d.el.offsetTop, '--spring-settle');
      if (land) {
        // stays above the cards it flies over until it has landed (Home.svelte)
        d.el.dataset.landing = '';
        const off = () => delete d.el.dataset.landing;
        land.addEventListener('finish', off);
        land.addEventListener('cancel', off);
        fly(d.el, land);
      }
      if (keep) {
        const ids = o.order();
        o.drop(d.id, ids, ids.join() !== d.start.join());
      }
    }

    function down(e: PointerEvent) {
      const grip = (e.target as Element).closest?.<HTMLElement>('[data-grip]');
      if (!grip || drag || !e.isPrimary || e.button !== 0) return;
      e.preventDefault(); // no text selection, no emulated mouse events
      const d = lift(grip, e.pointerId);
      if (!d) return;
      bento.setPointerCapture(e.pointerId); // on the bento: the card itself moves in the DOM, which would drop capture
      const p = local(e);
      d.gx = p.x - d.el.offsetLeft;
      d.gy = p.y - d.el.offsetTop;
      addEventListener('keydown', esc, true);
    }

    function move(e: PointerEvent) {
      const d = drag;
      if (!d || e.pointerId !== d.pointer) return;
      const p = local(e);
      const hit = cards().find((c) => c !== d.el && inside(c, p))?.dataset.card ?? null;
      if (hit && hit !== d.last) {
        const ids = o.order();
        shift(moveItem(ids, ids.indexOf(d.id), ids.indexOf(hit)), d.el);
      }
      d.last = hit;
      d.tx = p.x - d.gx - d.el.offsetLeft;
      d.ty = p.y - d.gy - d.el.offsetTop;
      follow(d);
    }

    const up = (e: PointerEvent) => drag && e.pointerId === drag.pointer && finish(e.type === 'pointerup' || e.type === 'lostpointercapture');

    function esc(e: KeyboardEvent) {
      if (e.key !== 'Escape' || drag?.pointer == null) return;
      e.preventDefault();
      e.stopPropagation();
      finish(false);
    }

    function key(e: KeyboardEvent) {
      const grip = (e.target as Element).closest?.<HTMLElement>('[data-grip]');
      if (!grip) return;
      if (!drag) {
        // a held Space repeats: only the first press picks up (else hold = pick up, then drop)
        if (e.key === ' ') (e.preventDefault(), e.repeat || lift(grip, null));
        return;
      }
      if (drag.grip !== grip || drag.pointer != null) return;
      const step = STEP[e.key];
      if (step) {
        e.preventDefault();
        const ids = o.order(), i = ids.indexOf(drag.id);
        if (i + step < 0 || i + step >= ids.length) return;
        shift(moveItem(ids, i, i + step));
        grip.focus(); // Svelte may have moved this card's node, which drops focus
      } else if (e.key === ' ' || e.key === 'Enter' || e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        if (!e.repeat) finish(e.key !== 'Escape');
      }
    }

    // The window loses focus or the tab hides mid pointer drag (Alt+Tab, a notification): the release may never come,
    // so it is cancelled like Esc. A keyboard pick-up is left to focusout.
    const lose = () => drag?.pointer != null && finish(false);
    const hidden = () => document.hidden && lose();

    // Tabbing away from a picked-up card drops it where it is
    const blur = (e: FocusEvent) => !moving && drag?.pointer == null && e.target === drag?.grip && finish(true);

    bento.addEventListener('pointerdown', down);
    bento.addEventListener('pointermove', move);
    bento.addEventListener('pointerup', up);
    bento.addEventListener('pointercancel', up);
    bento.addEventListener('lostpointercapture', up);
    bento.addEventListener('keydown', key);
    bento.addEventListener('focusout', blur);
    addEventListener('blur', lose);
    document.addEventListener('visibilitychange', hidden);
    return () => {
      bento.removeEventListener('pointerdown', down);
      bento.removeEventListener('pointermove', move);
      bento.removeEventListener('pointerup', up);
      bento.removeEventListener('pointercancel', up);
      bento.removeEventListener('lostpointercapture', up);
      bento.removeEventListener('keydown', key);
      bento.removeEventListener('focusout', blur);
      removeEventListener('blur', lose);
      document.removeEventListener('visibilitychange', hidden);
      removeEventListener('keydown', esc, true);
      // destroyed mid-drag (navigation): undo our marks, keep the order as it is, save nothing
      if (drag) {
        drag.follow?.cancel();
        delete drag.el.dataset.lifted;
        drag.grip.setAttribute('aria-pressed', 'false');
        if (drag.pointer != null && bento.hasPointerCapture(drag.pointer)) bento.releasePointerCapture(drag.pointer);
        drag = null;
      }
      for (const a of flights.values()) a.cancel();
      flights.clear();
    };
  };
}
