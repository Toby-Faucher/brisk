# Blueprint

Cyanotype drafting sheet. Share Tech Mono caps for everything, Architects Daughter for the red-pencil notes.

- Layer.svelte: sheet frame with zone marks, title block (sheet no, REV C, scale, page name as drawing title, live cursor
  X/Y), a pencil note per page above the host pill, a faint crosshair. Hidden on /stream, pointer-events none.
  It also hands each card its real width: a ResizeObserver sets `--dw`, CSS counts it up as `NNN PX` on the dimension line.
- Cards redraw their outline on load (registered `--bp-p`), dimension lines wipe in, numbers tick (registered `--bp-n`).
- Covers: grayscale, high contrast, screen-blended onto the paper, hatch overlay, plate numbers (PL.01) in pencil.
- Accent: the user's accent colours primary buttons, the current-page bar, progress bars and focus marks. Everything else is one blue.
- Reduced motion: all animation off, crosshair hidden.

## Needs shared change

None.

## Known gaps

- The live GhostBar/HUD could not be rendered (needs a real WebTransport stream); it is styled from its CSS only.
- Home card height dimension lines are not drawn, only widths.
