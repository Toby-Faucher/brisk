# Zine

Photocopied black on off-white, one pink and yellow spot colour. Ransom-note nav (each item its own font, tilt and colour),
masking-tape strips on cards and covers, torn edges (hero, toasts), grain from an inline SVG, stop-motion `steps()` jitter
on hover. Covers are B/W photocopies glued with tape; hover peels the tape and brings the colour back.

Fonts (self-hosted, exact pins already in package.json, all 5.3.0): `@fontsource/special-elite` (body), `permanent-marker`
(notes), `bebas-neue` (buttons, card titles), `dm-serif-display`, `bowlby-one` (page titles).

`Layer.svelte`: a marker note per route, two staples and an "Alt+U new zine" stamp. Static, aria-hidden, not on /stream.

Accent: the spot colour. Default (mint) and Pink both give hot pink; the others swap it for a bright ink (indigo, orange,
lime, sky). Text on it is always black. Yellow stays fixed.

Known limits: headings can't get per-word ransom styling in pure CSS (only the first letter). A live GhostBar could not be
rendered in the mock harness, so its CSS is untested visually.

Needs shared change: none.
