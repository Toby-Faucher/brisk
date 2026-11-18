# Tarot

Mystic tarot deck: indigo velvet, gold foil linework. Dark only. Files: `theme.css` (tokens, medallion nav, frames, cards,
dialogs, stream chrome) and `Layer.svelte` (star dust, a slowly turning zodiac wheel, a fortune line).

- **Nav**: the sidebar is a fixed top row of round arcana medallions with a Roman numeral above and the label below (CSS
  counter). Hover flips the medallion. "Coming" items get a dashed ring and a "soon" tag. The host pill is a gold plaque at the
  right end; its menu opens downward.
- **Cards**: panels get a gold border, an inner hairline and four corner brackets. Game covers become tarot cards: gold frame,
  inner frame, Roman numeral on a dark ribbon, and a flip animation (`tr-flip`) on hover or focus.
- **Type**: Cinzel for titles, labels and buttons, Cormorant Garamond for body text. Titles use a gold foil gradient.
- **Layer**: star dust, a 360 s zodiac wheel with a constellation, a rare shooting star, and a one-line fortune after each page
  change ("Small changes turn the wheel."). Not mounted on `/stream`; static under reduced motion.
- **Accent**: tints focus rings, the active medallion glow and hover glows. Buttons stay gold foil on purpose.

Known gaps:
- Card numerals count the cards mounted in a row, so they restart when a long shelf virtualizes.
- The Running badge and Quit button sit close on narrow covers.
- `axe` reports `label-content-name-mismatch` on game cards; the original look has the same finding with the mocked API.
