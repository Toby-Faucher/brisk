# Versus (`versus`)

A fighting-game select screen. Hot orange against blue, a dark lightning seam down the middle, drifting speed streaks.

- Fonts: `@fontsource/black-ops-one@5.3.0` (nav, buttons, labels) and `@fontsource/bangers@5.3.0` (page titles, banners,
  numbers). Body text keeps the app font so long text stays readable.
- The sidebar becomes the top bar: two slanted health bars split at the middle with a round timer (all drawn by the Layer
  behind the real controls). Left bar holds the nav (current page is a gold slab), right bar holds the host pill; its
  machine menu drops down. "Coming" items are hidden here. Buttons, chips and tabs are skewed slabs with block shadows.
- Library is the character select: covers are framed tiles, the hovered or focused card gets a flashing cursor frame with
  corner brackets and a "Select" button, and a big portrait with a name plate and two bars fills the right column.
  "Show all" turns a shelf into a real grid.
- Layer: round banner and a "VS" splash with a lightning flash on every page load or change (about 2 s, "Round one",
  "Round two", ... with a line per page), a round timer that counts down from 99 on each page, a hit on the left bar
  after the splash, tiny win stars for earlier rounds, and a combo counter at the right: toasts that arrive within 4.5 s of
  each other count as a combo.
- Dialogs and the Ctrl+K menu get a white frame, a gold cap line and an orange/blue split backdrop. Toasts are gold slabs.
- Stream chrome: restyled only (framed bar and card); no Layer and no top bar on /stream.
- Accent: the setting drives the primary buttons and the cursor frame colour. Everything else is fixed orange, blue, gold.
- Narrow screens (900px and less): the bars and portrait go away, the nav becomes a plain sticky strip.
- Reduced motion: no splash, a still timer, no streaks, no flashing cursor.

## Known gaps

- Portrait and bars are decorative; power and speed bars come from the name length, not game data.
- The portrait pane only follows pointer and focus on the Library page. First card shows until you move.
- `vs-pane` copies the card's cover (image or gradient) by reading the DOM, so it follows the card's art but not tags.
- `label-content-name-mismatch` on game cards fails in the original look too.

## Needs shared change

Nothing.
