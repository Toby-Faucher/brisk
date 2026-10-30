# Block world (blocky)

A voxel sandbox look: chunky bevelled slabs, pixel type, textures drawn as tiny SVG pixel grids (dirt, grass, stone).

- Font: `@fontsource/silkscreen@5.3.0` (400 and 700). It is capitals only, so everything reads as upper case.
- The sidebar drops to the bottom as a hotbar: a grass block (brand), one numbered slot per page, "Coming" items as dim
  slots, and the host pill as a stone slot on the right. The current page gets the white selector frame.
- The page sits on dirt. Titles are wooden signs. Cards, charts, dialogs and menus are grey inventory panels with a light
  bevel top left and a dark one bottom right. Chips, covers and inputs are sunken slots (the bevel flipped).
- Buttons are stone slabs; the main button takes the accent colour. Progress bars are striped green XP bars.
- Toasts become a chat log at the bottom left, each line prefixed with `<Brisk>`.
- Layer: a sky strip at the top that follows the clock (square sun by day, moon and stars by night, drifting blocky
  clouds, a grass line), an XP bar and level above the hotbar (level = total hours played divided by 10), and the page
  name that pops up over the hotbar after each page change. All decoration, `aria-hidden`, not mounted on /stream.
- Text on the dirt is white and text on grey panels is dark: the page flips `--fg`, `--muted` and `--dim`, and panels flip
  them back.
- Reduced motion: clouds stand still, no transitions.

## Known gaps

- The accent setting only colours main buttons and the stream chrome accents, not the grass or XP green.
- The XP bar and level sit over the bottom of the page content; they are small, but cards scroll under them.
- Under 900px the hotbar shows icons only and does not wrap; at 390px it fits just.
- Silkscreen is wide, so long labels (module names, game titles) clip with an ellipsis sooner than in other styles.

## Needs shared change

Nothing.
