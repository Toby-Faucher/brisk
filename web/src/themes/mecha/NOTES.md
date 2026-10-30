# Mecha (`mecha`)

Sci-fi cockpit HUD: matte graphite, safety orange for alerts and hazard stripes, ice-blue readouts.

- `theme.css`: tokens, chamfered panels (`clip-path` plus a drawn diagonal edge), corner marks on cards, hazard-stripe page headers
  and dialogs, stencil labels ("CAUTION //", "STANDBY //", "NO SIGNAL"), nav as numbered mode selectors on a data rail,
  square status lamps (green ready, orange busy, amber blink off, red fast blink down), targeting-reticle corners on cover hover,
  stepped "clack" entrances. Command menu, dialogs, toasts, tooltips and the Stream bar, stats and menus are restyled too.
- `Layer.svelte`: top HUD strip (mode name, real host/machine lamp and state, session clock), viewport corner marks, a radar
  (only when the window is 880px tall or more), a scan line per page change, and a one-time boot screen per tab session.
  Everything is `aria-hidden` and `pointer-events: none`, and renders nothing on `/stream`. The boot screen and the animations
  are off under `prefers-reduced-motion`.

## Fonts

`@fontsource/rajdhani` (body), `@fontsource/chakra-petch` (stencil labels, headings, buttons), `@fontsource/share-tech-mono`
(readouts). All already in package.json, latin subsets only.

## Accent

The stock mint swatch maps to safety orange here (`:root[data-ui='mecha'][data-accent='mint']`). Every other swatch is honoured
for active states, buttons, lamps, corner marks and focus rings. Hazard stripes in the page header stay orange (alert colour).

## Known limits

- The Accent card still shows the mint swatch as teal while the style paints it orange.
- Machine card colours (user-chosen per machine) are not recoloured.

## Needs shared change

Nothing.
