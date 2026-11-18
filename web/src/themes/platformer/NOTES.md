# Platformer

A bright side-scrolling world. Lilita One type, original shapes only.

- The page is a dark framed stage over a sky. The sky (Layer) has drifting clouds, two rows of hills, bushes and a flag pole.
  A ground strip of bricks runs along the bottom.
- The sidebar is a green warp pipe with a lip on top. Nav icons are gold blocks that hop on hover. The host menu and dropdowns are pipes too.
- Cards are brick frames around a dark panel. Big numbers (`.big`) are gold question blocks. Page titles are cream signs.
- A HUD strip across the top (Layer, display only): score, a spinning coin counter, world number (changes per page), time, stars.
  Score goes up on each page change. Pressing a button, chip, tab or nav link bumps it like a block and pops a coin.
- The accent setting recolours the gold of buttons and blocks only a little (the accent token is used by focus and sliders).
- Reduced motion: no clouds, no coin spin, no bump, no pop, no hover hop.
- Narrow windows: the HUD hides the stars.

## Known gaps

- Hills and clouds are mostly covered by the stage; they show at the edges and under it.
- HUD numbers are decoration (`aria-hidden`), score is not saved.
- `label-content-name-mismatch` on game cards also fails in the original look.
- /stream gets chrome styling only (buttons, bar, glass). Not checked against a live bridge.

## Needs shared change

Nothing.
