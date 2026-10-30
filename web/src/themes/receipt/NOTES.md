# Receipt

Thermal-printer receipts on a dark desk. Light only. Files: `theme.css` (everything) and `Layer.svelte` (desk extras).

- **Paper**: the sidebar is a narrow "MENU" strip, `main` is the receipt. Both are paper with zigzag torn edges, drawn as
  background gradients (no mask, so menus and dialogs are never clipped). `main` gets a header ("BRISK MARKET", terminal,
  order number, time) from `::before` and a barcode plus "THANK YOU, COME AGAIN" footer from `::after`, in faded grey.
  The stamp and order number come from `--rc-stamp` / `--rc-order`, set by `Layer`. Not on `/stream` (its `main` has no
  `.page-in`); there only the chrome (panels, buttons, menu strip) is paper.
- **Nav**: items are receipt lines with a dot leader and a line number. Hover inverts to ink, the current page is
  highlighted with the accent. "Coming" items are struck through. The host pill is the last line of the strip.
- **Type**: Courier Prime 400 and 700 (`@fontsource/courier-prime` 5.3.0).
- **Item lines**: `dl` rows (Machines, About, shortcuts) get dot leaders. History is a list of lines, play time as `QTY`.
- **Covers** print in grey like a thermal printout; colour comes back on hover or focus.
- **Buttons** are stamps: ink border, hard shadow, accent fill. Ghost buttons are dashed. Checkboxes are `[X]` boxes.
- **Dialogs** and the Ctrl+K menu are paper slips with torn edges.
- **Layer**: a printer bar fixed at the top (status text changes per page), a shift slip with a barcode and a faint coffee
  ring on the desk (wide screens only). Nothing is mounted on `/stream`; all of it is `pointer-events: none`.
- **Printing**: each page load feeds the receipt out top to bottom (`clip-path` with `steps()`), and blocks sweep in left
  to right. Off under `prefers-reduced-motion`.
- **Accent**: highlighter colour for the current nav line, buttons, chips, tabs, selection and focus glow.
- **Narrow screens** (under 1000px): the menu strip stacks above the receipt.

## Known gaps

- The slip and coffee ring are hidden under 1360px wide.
- Pages where the original hard-codes pale greys need a per-selector fix (done: card titles, charts, ring, controller
  numbers, hero). A new page with the same habit would show pale text on paper.
- Game art in the hero and detail header stays dark with light text; other dark overlays on art are left as they are.
