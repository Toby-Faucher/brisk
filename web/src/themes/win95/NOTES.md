# Classic desktop (win95)

- `theme.css`: teal desktop, grey beveled windows, navy title bars (drawn on `.card > h4`, the main pane and the
  dialogs with CSS only, the min/max/close glyphs are a background image and carry no text), pixel font, flat chips,
  property-sheet tabs, grey scrollbars, marching-ants focus on buttons.
- `Layer.svelte`: the taskbar (Start button, current page task, host state and clock) and the Start menu. The menu lists
  the real nav links, a Run entry that opens the Command menu (Ctrl+K) and the host state. Not mounted on `/stream`.
- The original sidebar stays as an "Explorer" pane on the left, so every link and the host pill are in two places.

## Font

- `@fontsource/pixelify-sans@5.3.0` (latin 400 and 700), imported from `theme.css`.

## Needs shared change

Nothing.
