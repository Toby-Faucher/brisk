# Dark bento

Near-black tiles on a black ground, 12px gaps everywhere, the sidebar is a tile too. Each tile has a 1px gradient border
that is lit along the top edge in the accent colour, a faint inner glow, and a spotlight that follows the pointer across
tile borders (Layer.svelte sets `--sx/--sy` per tile; only tiles within 320px of the pointer are touched). Cards lift and
glow on hover. Tiles enter one by one, with a brief brightness flash. Big numerals use Outfit, tiny labels use Geist Mono
in uppercase. The accent setting drives the edges, glow, buttons and current nav item, so it is very visible.

Fonts: `@fontsource-variable/outfit` (imported), Geist and Geist Mono from the base look. Dark only.

## Verified
axe on every view (1440x900, mocked API, pink and mint accent): only the known `label-content-name-mismatch` on game cards.

## Known gaps
- Spotlight is off under reduced motion and on touch.
- Library game tiles are not spotlit; only `.card`, sidebar, hero, figures and `.rs`.
- Below 800px the sidebar just shrinks to the base layout.
