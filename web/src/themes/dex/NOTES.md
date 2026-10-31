# Creature dex

A red handheld encyclopedia. Files: `theme.css`, `Layer.svelte` (the button pad).

- **Shell**: red glossy plastic page, a hinge seam, and the page content sits in a dark green LCD inside a grey bezel with a
  slow scan band and scan lines. Orbitron for headings, labels and buttons; body text stays system sans for reading.
- **Sidebar**: a blue lens that blinks, a "BRISK DEX" signboard, entries as mini screens numbered "No. 01" and up
  (CSS counter). The current page is a lit green entry. Coming items are dashed and say "(not caught yet)".
- **Pad** (`Layer.svelte`, hidden under 800px height and on /stream): D-pad up/down walk the entry list, left/right are
  Back/Forward, A opens Ctrl+K, B sends Escape. All have tabindex -1; the real keys still work.
- **Library**: installed games show a capsule ball ("caught"); not installed games are dark silhouettes with "???" and
  fade to colour on hover or focus. A running game's ball wobbles.
- **Accent**: used for buttons, selected chips and selection. Dark only.
- **Motion**: lens blink, scan band, ball wobble. All off under prefers-reduced-motion.

Known gaps: no per-card "No." numbers (the Library shelf is virtualised, a CSS counter would be wrong); /stream has only
restyled chrome, no frame; under 900px the shell collapses to a stacked layout that was not tuned.
