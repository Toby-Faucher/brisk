# CRT arcade (`crt`)

Phosphor green on black, pixel type, 2px hard borders, no rounded corners.

- `theme.css`: tokens, nav as a numbered menu with a selection bar, hard-edged buttons that invert on hover,
  duotone covers, dialogs, command menu, toasts and tooltips. Motion uses `steps()` only (power-on and boot reveals).
- `Layer.svelte`: scanlines with a slow roll, vignette, a faint flicker, an INSERT COIN footer with a rotating tip line,
  and the SVG filter `#crt-duo` (green duotone plus 4-level posterize) that theme.css applies to covers.
  The whole layer renders nothing on `/stream`. Flicker, roll, blink and typing animations are off under reduced motion.

## Fonts

- `@fontsource/press-start-2p@5.3.0` (headings, nav, buttons)
- `@fontsource/vt323@5.3.0` (body)

Both are imported from theme.css (latin subset only). Body text uses `font-size-adjust: 0.56` so VT323 reads at a normal size.

## Known limits

- The Accent swatches in Settings have no effect here: the style is one hue on purpose.
- The brand wordmark image is hidden and replaced with text drawn in CSS.

## Needs shared change

Nothing.
