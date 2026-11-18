# Cozy town

Village-game look: cream and mint picnic cloth, wooden signs with scooped corners and nails, soft pillow buttons.

- Header: the sidebar becomes a row of signposts standing in a meadow. The sky strip (Layer.svelte) follows the local time:
  sun by day, moon and stars by night, dawn and dusk colours between. It refreshes once a minute.
- Layer extras: a few drifting clouds, five falling leaves, and a small neighbour (bottom right) that says a line per page.
  All CSS motion, pointer-events none, hidden on /stream, off with reduced motion (the global rule stops the animations).
- Game covers hang as framed paintings on a string and swing when hovered.
- Toasts are speech bubbles with a tail. Host pill is a mailbox with a flag.
- Accent: honoured. `--accent` is a deep tone of each hue so cream text on it stays AA.
- Fonts: Fredoka (headings, buttons), Nunito (body), Patrick Hand (notes, chips, toasts).

## Needs shared change

None. Known limits: below 1560px the "Coming" signposts shrink to icons; the Coming label is hidden at every width.
