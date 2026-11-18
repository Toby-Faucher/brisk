# Persona

Red, black and white with sharp diagonal cuts. Original art only; the copy is our own. Files: `theme.css` (everything) and
`Layer.svelte` (background shards, calling card).

- **Menu**: a tall black sidebar with a skewed, red-edged right side. Items are italic Anton, tilted. The page you are on is a
  red slash, hover is a white slash that pops with an overshoot. Items fly in on load. The host pill is a white tab at the foot.
- **Page change**: two jagged panels (black, then red) wipe across the screen, and a white calling card with the page name and a
  one-line tag flies through the middle. Both are skipped under `prefers-reduced-motion` and never run on `/stream`.
- **Panels**: cut with `clip-path`, a red left edge, white bottom edge, small alternating tilt that straightens on hover.
  Titles sit on a red slab over a white slab. Buttons and chips are slanted parallelograms.
- **Dialogs and menus**: tilt in from `-9deg` with a spring overshoot (`@starting-style`).
- **Cursor**: a white star, red over links and buttons. Swatches are diamonds.
- **Accent**: the default mint becomes persona red. Any other accent still tints focus rings and small accents.
- **Type**: Anton (headings, nav), Rubik (text).

Known gaps: the Library cover letter cards fail axe `label-content-name-mismatch` here, same as the original style on this branch
(not caused by this style). Large dialogs are rotated a little, so their edges can look cut by the clip. Backgrounds use `clip-path` on
many panels; fine on a desktop, untested on a weak phone.
