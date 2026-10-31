# Slate (sheikah)

A worn teal-grey stone tablet with thin glowing cyan rune lines and a little orange for warm hover states. Original art and copy.
Files: `theme.css` (everything) and `Layer.svelte` (background eye glyph, page-change scan).

- **Menu**: a bezel with cyan notches. Each item is an icon in a ring. Hover or focus lights the item cyan, turns the ring and sends out a ripple. Items light up in sequence on load.
- **Page change**: a cyan line sweeps across, a ring draws itself and a row of rune marks draws stroke by stroke. Skipped under `prefers-reduced-motion` and on `/stream`.
- **Panels**: slate with a double frame and four glowing corner ticks. Dialogs wake up from a bright, blurred state.
- **Accent**: the default mint becomes cyan. Warnings and ghost-button hover use orange.
- **Type**: Cinzel (headings), Rajdhani (text).

Known gaps: the home grid does not collapse on a phone (the app's own layout). The Library skeleton `aria-label` axe finding is
the same on the original style.
