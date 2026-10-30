# Ink war

Dark purple-black ground, hot orange against electric green and violet. Original art only. Files: `theme.css`, `Layer.svelte`.

- **Menu**: a dark pool on the left with a wavy, dripping orange edge. The page you are on is a fresh orange splat (it pops in),
  hover is a green one and the icon wobbles. Items bounce in on load. The host pill is a green blob at the foot.
- **Page change**: orange ink floods the screen from the middle (green right behind it), the page name sits on it in fat type,
  then it drains down and leaves tongues. Skipped under `prefers-reduced-motion`, never on `/stream`.
- **Panels**: blobby corners, a violet border and a hard orange or green shadow, small alternating tilt, a jelly wobble on hover.
  Page titles are an orange splat with drips and a green one behind.
- **Phones** (under 760px): the menu becomes a strip along the bottom.
- **Type**: Lilita One (headings, nav, buttons), Fredoka (text).

Known gaps: the phone strip scrolls sideways, so the host pill can sit off screen. The axe `.skel` aria-label failure on Library
and the stream degraded-state wait in `axe-check.mjs` also fail on the default style against this preview (not caused by this style).
