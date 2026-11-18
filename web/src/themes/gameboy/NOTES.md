# Handheld (Game Boy)

The screen is the app. Every colour on it is one of four olive greens (`#0f380f #306230 #8bac0f #9bbc0f`). On desktop
(900px wide and 600px tall or more, not `/stream`) a grey plastic shell surrounds the screen: bezel, "DOT MATRIX WITH
STEREO SOUND", a red battery LED that dims for a beat every five seconds, and a speaker grille. Under that size, or on
`/stream`, the shell is gone and only the green palette stays on the chrome.

## How it works

- `Layer.svelte` holds an inline SVG with two filters. `gb-lcd` snaps any colour to the four greens by luminance and is
  applied to the whole `.app` (so cover art, icons, gradients and translucent layers all end up in the palette) and to
  portalled dialogs, the command menu, toasts and the stream chrome. `gb-cover` brightens first, then posterizes into
  four equal bands, for cover art. CSS dither tiles sit on top of covers.
- The shell buttons are real. D-pad = arrow keys (spatial focus move when the page does not handle the key), A = Enter
  (also clicks the focused link or button), B = Escape (goes back when nothing is open), Start = Ctrl+K, Select = Alt+U.
  They are `tabindex=-1` and cancel `mousedown`, so a click never steals focus from the screen. A real key press lights
  the matching button.
- `.app` is fixed to the screen rectangle and `main` scrolls inside it. Big panels (`.gd` game detail, `.mm` machine
  modal) are pinned to the same rectangle.
- Motion is stepped: `steps(2)` fades, a 6-step wipe on page change, a bobbing menu cursor, blinking "PRESS START",
  "NOW LOADING..." and "GAME OVER" empty states. Power-on logo scrolls down once per tab session (no sound, so no ding).
  Reduced motion turns it all off, including the logo.
- Easter egg: up up down down left right left right B A.

## Fonts

`@fontsource/silkscreen` (labels, nav, buttons), `@fontsource/press-start-2p` (headings), `@fontsource/pixelify-sans`
(body). All already pinned in package.json.

## Accent

Ignored on purpose. The four greens are the point. The Accent card in Settings still works for the other styles; here
it shows dither tiles and a line that says so.

## Contrast

Text is only `#0f380f` on `#9bbc0f` / `#8bac0f` (5.0 to 6.0:1) or `#9bbc0f` on `#0f380f`. `#306230` is for borders and
shadows only. Dimmed things (soon cards, locked achievements, disabled buttons) use dither, not opacity.

## Known issues

- Dialogs that are not game detail or machine modal (command menu, confirms, lightbox) are centred on the viewport, so on a
  desktop they float over the plastic.
- The pixel-grid glass over the screen is drawn above the app, so it tints every pixel slightly (about 7%).
- Could not drive `/stream` into a connected state (no WebTransport). The GhostBar was checked with the bar forced
  visible in the failed state; its quality and clip menus were styled but not seen open.

## Needs shared change

Nothing required. Optional: `data-ui` on `.cm`/`.rs` dialogs so they could sit inside the screen rectangle.
