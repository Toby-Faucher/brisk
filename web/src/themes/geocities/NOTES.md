# Web 1.0 (geocities)

- `theme.css`: a 1997 home page. Starfield body (CSS gradients, no images), cream dotted main pane with a pink ridge,
  white table-style cards with ridge borders and accent title bars, grey beveled buttons, rainbow rules, blue underlined
  links that turn purple when visited, hazard tape on every "soon" item (sidebar and Home cards), chunky scrollbars.
- `Layer.svelte`: a ticker on top (real host state and game count), a footer with "Best viewed in 1024x768", a decorative
  webring, a 7-digit counter showing the real game count, and a wobbling mail icon. All aria-hidden, pointer-events none,
  not mounted on `/stream`. It calls `watchGames()` so the count is filled on every page.
- Fonts: `@fontsource/comic-neue`, `@fontsource/bungee`, `@fontsource/vt323` (all already in package.json).
- Motion: ticker, blinking "NEW!" and "soon" tags, moving rainbow rules, mail wobble. All off under reduced motion.
- Accent: honoured. It colours card title bars, primary buttons and the selected settings tab. Text on it uses `--accent-fg`.
- The chrome of a live `/stream` (ghost bar, stats card) is styled from the class names; it was not seen rendered.

## Needs shared change

Nothing.
