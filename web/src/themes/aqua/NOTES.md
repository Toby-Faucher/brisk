# Aqua glass

Light glass in the style of Mac OS X 10.x. Files: `theme.css` (tokens, dock, cards, buttons) and `parts.css` (screens,
dialogs, fields, motion). No Layer.svelte: everything is CSS on the existing DOM.

- Sidebar becomes a magnifying dock at the bottom (the icon under the pointer grows, its neighbours a little). The wordmark
  sits in a menu bar across the top. The host pill and its menu live at the right end of the dock.
- "Coming" modules are hidden in the dock (they are not links; Home still shows their cards).
- Motion: cards pop in on the settle spring, dialogs drop in like a sheet and fall back toward the dock when closed, the
  Settings tab pill glides on the snappy spring. Default buttons in dialogs and the Library hero pulse. All of it is off
  under `prefers-reduced-motion`.
- Toasts move to the top right so the dock stays clear.

## Fonts

- `@fontsource-variable/open-sans` 5.3.0 (published 2026-07-19). Imported in `theme.css`.

## Needs shared change

Nothing.

## Known limits

- Dock icons are tinted by `nth-of-type`, so a new module in the nav gets the default blue-grey tile.
- Hero and game panel banners keep their dark cover colour wash (white text) on purpose.
