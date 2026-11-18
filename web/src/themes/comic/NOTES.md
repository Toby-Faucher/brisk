# Comic book

Newsprint paper, CMYK ink, halftone dots, caption boxes and speech bubbles. Fonts: Bangers (headings, buttons, nav) and Comic Neue (body).

- Side margin: cyan halftone strip, nav items are tilted caption boxes, the current page is an accent box with a "!" burst.
- Panels (`.card`): thick ink border, a halftone corner that fades out, a few skewed, a few nudged for uneven gutters.
- Host pill and toasts are speech bubbles with tails. Page headings and card titles are yellow caption boxes.
- `Layer.svelte`: a "MEANWHILE..." caption at the top of each page (scrolls away), a dog-ear in the corner, and sound
  effect bursts (POW!, WHAM!, ZAP!...) that pop above a primary button when it is pressed, with a 2px page jolt.
  No words and no jolt under reduced motion; nothing is drawn on /stream.
- Motion: panels slam in with a small overshoot, pages turn in (clip + perspective on `main`, never on /stream).

## Accent

Honoured: `--accent` (a dark variant per hue, so white text passes AA) fills primary buttons, the current nav item,
selected tabs and chips, and checked controls. Yellow, magenta and cyan are fixed print inks.

## Known issues

- The dog-ear sits over the bottom-right corner of the page.
- Tooltips use one tail-less bubble (they flip, so a tail could point the wrong way).

## Needs shared change

None.
