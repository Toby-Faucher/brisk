# Transit

A subway map and station signage. Near-black signs, white frames, coloured line bullets, Overpass type (a Highway Gothic cousin).

- The sidebar is a metro line. Each nav item is a stop with a coloured bullet on a line in the accent colour. The current page is a
  white station plate. "Coming" modules sit on a dashed stretch of line. A tactile paving strip runs down the platform edge.
- `Layer.svelte` draws a signboard across the top: the current stop, "Next stop" (read from the sidebar, so a new module shows up by
  itself), a rolling announcement tape and a split-flap clock. Hidden on /stream. Display only (`pointer-events: none`).
- Cards are signs with a top bar in a line colour (cycles through six). Page titles are name plates. Dialogs and menus are black signs
  with a white frame. Buttons are white pills with a hard shadow.
- Departures board look: big numbers (`.big`) and the History duration row are yellow on black split-flap tiles that flip in.
- The accent setting recolours the line and the signboard rule.
- Reduced motion: no flip, no tape roll, no hover swell.
- Narrow windows (800px and less): the sidebar keeps only the stops.

## Known gaps

- Overpass sets a middle dot with tight spacing, so "a · b" reads "a ·b" in a few meta lines.
- The live /stream chrome (ghost bar) was not looked at: headless has no bridge. It only inherits the global button, glass and focus styles.
- Locked achievements in the game panel are lifted from opacity .6 to .78 to keep AA on the black panel.
- The announcement lines and "Next stop" are not announced to screen readers on purpose (`aria-hidden`).

## Needs shared change

Nothing.
