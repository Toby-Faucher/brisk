# Menu Quest (jrpg)

A 16-bit JRPG menu screen. Deep-blue gradient windows with white double borders and a hard shadow, gold highlights, a
night sky with faint stars, DotGothic16 type.

- Cards, dialogs, the Ctrl+K menu, toasts and the stream chrome are all "windows". Cards and dialogs open with a stepped
  wipe from a line.
- The sidebar is one tall window. A white pointing hand bobs beside the hovered, focused or current item. The Command
  menu has the same hand on the selected row.
- Home: the host card's GPU and CPU gauges are HP-style bars (green fill from the temperature).
- Library: the hero card carries an "A game appears!" plate; hovered game names get a triangle cursor.
- `Layer.svelte`: a dialogue box at the bottom with typewriter text, a short tip per page, a blinking arrow when done,
  gone after a few seconds. Decorative (`aria-hidden`, `pointer-events: none`), never on /stream, hidden under 800px.
- Accent: the default (mint) keeps gold. Any other accent recolours the gold highlights.
- Reduced motion: no wipe, no bobbing hand, no typing (the tip appears whole).

Font: `@fontsource/dotgothic16`.

## Known gaps

- Under 800px the menu becomes a strip on top, but Home's card grid is not responsive in any style, so cards get narrow.
- The Machines card rings (SVG) keep their round shape; only Home's gauges became bars.
- The Home axe check at 390px reports one target-size item (overlapping narrow cards); 1440px passes on every view.
