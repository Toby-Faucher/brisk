# Poster wall

Cover art is the interface. Near-black ground, warm paper white, condensed all-caps poster lettering, film grain.

- Top bar: the sidebar collapsed into a thin transparent strip of small caps. It gains a solid ground once the page scrolls
  (scroll-driven animation, no JS). The host pill sits at its right end; its menu drops down.
- Library: a full-bleed hero, then poster rows with zero gutters. A card shows only its cover; the name and Details appear
  over a dark gradient on hover or focus. Shelf cards keep the 152 px pitch that `Shelf.svelte` uses for its windowing.
- Home: hairline tiles, Resume as a full-width marquee. History and Machines are hairline mosaics.
- Command menu, dialogs, toasts, tooltips: square, black, poster lettering.
- `Layer.svelte`: grain and vignette, off on `/stream`.
- The "Coming" items (Agents, News, Workouts) are disabled placeholders and are hidden in this style.

## Fonts

- `@fontsource/bebas-neue@5.3.0` (latin 400 only). Body text keeps the app's Geist.

## Needs shared change

Nothing.
