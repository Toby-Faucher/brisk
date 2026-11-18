# maze (Maze chase)

A maze-chase arcade cabinet. Black field, crisp neon-blue double-line walls, rows of pellet dots. No scanlines or blur.

- `theme.css`: tokens, Press Start 2P headings and nav, cards/dialogs/menus as double-line walls, pellet dividers under page
  titles, yellow pill buttons. The active nav item is a pulsing power pellet; "Coming" items get a small blue ghost.
- `Layer.svelte`: outer wall and pellet track around the window, a black mask so scrolling content slides under the rim,
  a SCORE / HIGH SCORE / CREDIT header (the score only ticks, it means nothing), a yellow wedge and four ghosts that
  patrol the rim, and READY! blinking after each page change. All `pointer-events: none`, nothing on `/stream`.
- Accent: the default mint accent becomes yellow. Other accents work as set.
- Reduced motion: no ticking, no READY!, actors parked on the rim.

Known gaps
- The accent swatches in Settings still show mint, which maps to yellow here.
- Under 900px wide the sidebar stays a column, so the page is cramped.
- The wedge and ghosts use `offset-path: inset()` (Chrome 116+, Firefox 122+, Safari 16+).
- `label-content-name-mismatch` on game cards fails in the original look too.
