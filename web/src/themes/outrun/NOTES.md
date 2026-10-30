# Outrun (`outrun`)

Synthwave sunset: night purple sky, striped sun, a perspective neon grid that scrolls toward you, chrome italic
Orbitron headings, nav items as neon sign tubes that flicker on at load.

- `theme.css`: tokens, type, tube buttons, glass panels with neon edges, framed covers, dialogs, command menu, toasts,
  tooltips, the Stream idle screen (own sunset and grid) and the ghost bar HUD (a tube strip).
- `Layer.svelte`: sky, stars, sun, scrolling grid floor, two palms, and a bottom ticker with a tiny car driving along it.
  All `aria-hidden`, `pointer-events: none`, renders nothing on `/stream`. Grid, ticker, car, stars and nav flicker stop
  under `prefers-reduced-motion`. The grid moves with a transform only (no per-frame layout or JS). In Chromium the sun
  sinks as you scroll (CSS scroll timeline, `@supports` guarded).

## Accent

The accent drives the horizontal grid lines, nav tube 1, focus rings, primary buttons (accent to magenta gradient), covers'
inner frame and the Stream idle grid. Magenta (`--mag`) and violet are fixed second hues, so with the pink accent the
style is close to single-hue.

## Fonts

`@fontsource-variable/orbitron` (already in package.json) for headings, nav, buttons, tooltips. Body stays Geist.

## Known limits

- The decorative ticker strip covers the bottom 28 px of the window; `main` and the sidebar leave room for it.
- Real live video on `/stream` was not rendered (needs a WebTransport host); the idle, failed and ghost bar states were.
- Headings use gradient text (`background-clip: text`), which axe cannot measure; colours were chosen by eye.

## Needs shared change

Nothing.
