<script lang="ts">
  import { onMount } from 'svelte';

  // Tiles read --sx/--sy (the pointer, in the tile's own pixels) to light their border and glow.
  // Only tiles within reach are updated, one rAF per move, nothing at all under reduced motion.
  onMount(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const REACH = 320;
    let x = 0,
      y = 0,
      raf = 0;
    const paint = () => {
      raf = 0;
      for (const el of document.querySelectorAll<HTMLElement>('.card, .side, .hero, figure, .rs')) {
        const r = el.getBoundingClientRect();
        const near = x > r.left - REACH && x < r.right + REACH && y > r.top - REACH && y < r.bottom + REACH;
        if (near) {
          el.style.setProperty('--sx', `${x - r.left}px`);
          el.style.setProperty('--sy', `${y - r.top}px`);
        } else if (el.style.getPropertyValue('--sx')) {
          el.style.removeProperty('--sx');
          el.style.removeProperty('--sy');
        }
      }
    };
    const move = (e: PointerEvent) => {
      if (e.pointerType === 'touch') return;
      x = e.clientX;
      y = e.clientY;
      raf ||= requestAnimationFrame(paint);
    };
    addEventListener('pointermove', move, { passive: true });
    return () => {
      removeEventListener('pointermove', move);
      if (raf) cancelAnimationFrame(raf);
    };
  });
</script>
