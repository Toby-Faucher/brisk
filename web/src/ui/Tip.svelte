<script lang="ts" module>
  // Chrome 125+; without it the tip is placed once from the trigger's rect (no flipping).
  const anchored = typeof CSS !== 'undefined' && CSS.supports('position-anchor: --a');
  const FALLBACK = {
    top: (r: DOMRect) => [r.left + r.width / 2, r.top - 6, '-50%, -100%'],
    bottom: (r: DOMRect) => [r.left + r.width / 2, r.bottom + 6, '-50%, 0'],
    left: (r: DOMRect) => [r.left - 6, r.top + r.height / 2, '-100%, -50%'],
    right: (r: DOMRect) => [r.right + 6, r.top + r.height / 2, '0, -50%'],
  } as const;
</script>

<script lang="ts">
  /**
   * Tooltip for the element just before it (`<button aria-label="…">…</button><Tip label="…" />`): a manual popover in
   * the top layer, CSS-anchored to that element, flipped at the viewport edge. Hover (300 ms) or keyboard focus shows it;
   * leave, blur, a press or Esc hides it. The trigger keeps its own name and gets aria-describedby (unless the tip only
   * repeats that name). On a trigger that cannot take focus it is hover-only: use it only for supplementary text.
   */
  let { label, placement = 'top' }: { label: string; placement?: keyof typeof FALLBACK } = $props();
  const id = $props.id();

  function attach(tip: HTMLElement) {
    const el = tip.previousElementSibling as HTMLElement | null;
    if (!el) return;
    el.style.setProperty('anchor-name', `--${id}`);
    // a tip that only repeats the trigger's name is not a description too (a swatch would read "pink, radio, pink")
    const describes = el.getAttribute('aria-label') !== label;
    if (describes) el.setAttribute('aria-describedby', id);
    let timer: ReturnType<typeof setTimeout> | undefined;
    const show = () => {
      clearTimeout(timer);
      if (tip.matches(':popover-open') || !el.isConnected) return;
      if (!anchored) {
        const [x, y, t] = FALLBACK[placement](el.getBoundingClientRect());
        tip.style.cssText = `left:${x}px;top:${y}px;transform:translate(${t})`;
      }
      tip.showPopover();
      document.addEventListener('keydown', esc, true);
    };
    const hide = () => {
      clearTimeout(timer);
      document.removeEventListener('keydown', esc, true);
      if (tip.matches(':popover-open')) tip.hidePopover();
    };
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && hide();
    const over = () => (clearTimeout(timer), (timer = setTimeout(show, 300)));
    const focus = () => el.matches(':focus-visible') && show(); // a click's focus does not pop a tip
    const on: [string, () => void][] = [['pointerenter', over], ['pointerleave', hide], ['pointerdown', hide], ['focus', focus], ['blur', hide]];
    for (const [k, f] of on) el.addEventListener(k, f);
    return () => {
      hide();
      for (const [k, f] of on) el.removeEventListener(k, f);
      el.style.removeProperty('anchor-name');
      if (describes) el.removeAttribute('aria-describedby');
    };
  }
</script>

<span {id} class="tip" class:anchored role="tooltip" popover="manual" style:position-anchor={anchored ? `--${id}` : undefined} style:position-area={anchored ? placement : undefined} {@attach attach}>{label}</span>

<style>
  .tip { position: fixed; inset: auto; margin: 0; border: 0; padding: 3px 8px; border-radius: 7px; background: #000d; color: #fff; font-size: 11.5px; font-weight: 500; line-height: 1.4; letter-spacing: 0; text-transform: none; white-space: nowrap; pointer-events: none; overflow: visible; opacity: 0; translate: 0 2px; }
  .tip.anchored { margin: 6px; position-try-fallbacks: flip-block, flip-inline, flip-block flip-inline; }
  .tip:popover-open { opacity: 1; translate: 0 0; }
  /* enter like a dialog, leave faster; display/overlay held by allow-discrete so the exit is seen in the top layer */
  @media (prefers-reduced-motion: no-preference) {
    .tip { transition: opacity var(--dur-exit) var(--ease-exit), translate var(--dur-exit) var(--ease-exit), display var(--dur-exit) allow-discrete, overlay var(--dur-exit) allow-discrete; }
    .tip:popover-open { transition-duration: var(--dur-tab); transition-timing-function: var(--ease); }
    @starting-style { .tip:popover-open { opacity: 0; translate: 0 2px; } }
  }
</style>
