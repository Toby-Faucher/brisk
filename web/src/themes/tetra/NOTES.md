# Falling blocks (tetra)

A falling-blocks puzzle game as a UI. Dark well, faint cell grid, seven glossy piece colours.

- Font: `@fontsource/audiowide@5.3.0` for headings, nav, buttons and labels. Body text stays a system sans for reading.
- Sidebar: a bezel with each page as a glossy block (colour cycles through the seven pieces). The current page glows and has a
  small piece stuck to its edge. "Coming" pages are dashed ghost pieces. The host pill is the HOLD box.
- Cards and the Library hero get a tetromino on two corners, a cell grid inside, and take a piece colour by position.
- Buttons are glossy blocks that hard-drop when pressed. Focus is a dashed ghost-piece outline.
- Page change: the page falls in, hits the floor and settles (`tt-drop`).
- Layer: a canvas behind the content (opacity .24). A greedy bot plays: pieces fall slowly, stack, flash and clear lines, with
  dashed ghost outlines where they will land. The sim lives at module level, so the stack survives page changes. Every page
  change also hard-drops one piece. On wide screens (1240px and up) a HUD on the right shows NEXT, LEVEL, LINES, SCORE and a
  short message. Not mounted on /stream.
- Reduced motion: no loop, a static stack, no drop animation, no ghost pulse.

## Known gaps

- The HUD hides below 1240px wide.
- The stream bar and popovers are only restyled through tokens; they were not seen with a live video.
- The sim redraws every frame while the tab is visible (about 300 cells, cheap, but not free).
- Game covers keep their own art; only the frame changes.
