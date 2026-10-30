# Boomer shooter (fps)

A 90s first-person shooter menu. Rust, brick and olive metal, all CSS: an SVG brick wall behind riveted steel panels,
Teko type in capitals, amber highlights.

- Layout: the sidebar becomes a top bar of weapon slots. Every nav link carries a number (1 to 9, from a CSS counter) and
  the current page is the lit slot. Number keys press the matching slot (not while typing, in a dialog or on /stream).
- `Layer.svelte`: a riveted status bar along the bottom with HEALTH, ARMOR and AMMO in big red numerals, three keycard
  slots and an original face that looks at the pointer, grimaces on a click and bleeds when health is low.
  - Page change: the screen jolts (Web Animations API on `main`), health drops a little, armor goes up on a new page.
  - Library, Machines and History hand out the blue, yellow and red keycards on the first visit.
  - Every click on a control fires a muzzle flash at the pointer and costs one round. At zero it reloads and heals.
  - State lives in module scope, so it survives the page remounts. It resets on a full reload.
- Crosshair cursor: green over the page, red ring over controls (text fields keep the I-beam).
- Buttons are brass plates (orange primary, olive ghost) that sink when pressed. Gauges on Home are red ammo bars.
- Accent: the default (mint) keeps amber. Any other accent recolours the amber highlights.
- Reduced motion: no jolt, no flash, no slam-in, no blinking message.

Font: `@fontsource/teko`.

## Known gaps

- The face mouth and brow changes use CSS `d: path()`, which only Chromium honours. Elsewhere the brows stay level.
- Nine slots fill the bar at 1440px. Under 800px the "soon" slots and the logo are hidden and the row wraps.
- `label-content-name-mismatch` on Library game cards fails in the original look too.
- The status bar numbers are decorative, not real host data.
