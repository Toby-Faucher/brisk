# Ink war

Splatoon-like: dark ground with a faint doodle pattern, hot pink against lime and violet. Original art only. Files: `theme.css`, `Layer.svelte`.

- **Menu**: a flat dark panel with a pink edge. The page you are on is a pink rounded tag, tilted a little; hover is lime.
  Items bounce in on load. The host pill is a lime tag at the foot.
- **Page change**: pink ink floods the screen from the middle (lime right behind it), the page name sits on it in fat type,
  then it drains down and leaves tongues. Skipped under `prefers-reduced-motion`, never on `/stream`.
- **Panels**: plain rounded boxes with a violet border and a hard pink or violet shadow, no tilt. Page titles are a skewed pink tag with a lime one behind.
- **Phones** (under 760px): the menu becomes a strip along the bottom.
- **Type**: Lilita One (headings, nav, buttons), Fredoka (text).

Known gaps: the phone strip scrolls sideways, so the host pill can sit off screen. The axe `.skel` aria-label failure on Library
and the stream degraded-state wait in `axe-check.mjs` also fail on the default style against this preview (not caused by this style).
