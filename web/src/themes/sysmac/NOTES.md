# Classic Mac

Pure black and white, System 6 style. No grey anywhere: every tone is a dither pattern.

- Font: `@fontsource/pixelify-sans@5.3.0` (400 and 700). Ligatures are off, because its "fi" and "fl" glyphs read as odd letters.
- The sidebar becomes the menu bar across the top: a happy Mac and "Brisk", the pages as menu titles (current one inverted),
  "Coming" items in italics, the host pill as the right-hand menu (its machine list drops down from it).
- The page is one window: striped title bar with a close box and a zoom box (both decorative), 1px border, rounded corners,
  hard drop shadow. The title follows the page (set by the Layer through `--sm-title`). Home and Settings cards are small
  windows with an inverted title bar. Dialogs get the striped title bar too.
- The desktop is a 50% dither. The Layer adds a clock in the menu bar, desktop icons on the right (Brisk HD, the three most
  recent games, all links) and a Trash can that wiggles and answers when you click it.
- Covers are 1-bit: grey, a Bayer pattern over it, a hard threshold (an SVG filter in a data URI). Plain `<img>` art
  gets a noise dither instead. Hover inverts a cover.
- Status dots are shapes, not colours: solid square = ready, dither = busy, hollow = down. Accent swatches are patterns.
- Cursor is a pixel arrow everywhere, a watch for about half a second after each page change (not with reduced motion).
- Nothing eases: transitions are off, the only animation is the Trash wiggle (stepped).
- Layer is not mounted on /stream. The stream chrome (bar, popovers, lost-connection card) is restyled as white 1-bit panels.

## Known gaps

- The accent setting does nothing here (1-bit has one colour). The swatches still select, so the setting is kept.
- Pixelify Sans digits are stylised: "5" reads a little like "S" and "2" like "z" at 15px.
- Charts keep their lines (black) but lose the area fill.
- On narrow screens the desktop icons hide and the "Coming" menu titles go away; the menu bar does not wrap.

## Needs shared change

Nothing.
