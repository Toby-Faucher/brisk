# Broadsheet

A Victorian newspaper front page. Light only. Files: `theme.css` (everything) and `Layer.svelte` (dateline, weather box).

- **Paper**: yellowed newsprint from an inline SVG noise tile plus a warm vignette. Ink is near-black, the second ink is a dark red.
- **Masthead**: the sidebar becomes "The Daily Brisk" in Playfair Display 900 (the real logo stays for screen readers), a
  ruled dateline (today's date, volume, issue number, set by `Layer` through CSS custom properties) and a section index strip
  for the nav. "Coming soon" modules sit in the strip as italic text. The host pill is the "Wire" at the strip's right end.
- **Layer**: a weather box and an edition box in the masthead's ears. Both change on each page change. Hidden under 1100 px
  and on `/stream`.
- **Headlines**: page titles are Playfair 900 under a double rule with a red kicker, and press in on load.
- **Cards** are boxed columns with a thick top rule and a small-caps head. Dialogs and menus are opaque paper with a hard offset shadow.
- **Covers** are engravings: grayscale, contrast, sepia and a halftone dot overlay. Hover or focus brings the colour back.
  Screenshots and achievement icons get the same treatment.
- **Game detail**: ruled section heads, a drop cap on the description.
- **/stream** is the night edition: the same type with the ink and paper swapped, on black.
- **Accent**: tints the red ink a little, checkbox and range colours, the sparkline lines. It cannot change the paper.
- **Motion**: headline press-in and the nav underline fade. Both off under `prefers-reduced-motion`.

Known gaps: the Home card grid overlaps its drag handles at 640 px (same as other styles). The Quick actions "Crisp" chip is
clipped there. Some Library covers read very dark with the halftone on a gradient fallback. No dark variant.
