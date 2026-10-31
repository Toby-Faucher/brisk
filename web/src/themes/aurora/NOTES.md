# Aurora

Spatial aurora: frosted glass over a drifting northern-lights sky. Dark only. Files: `theme.css` (tokens, dock, glass,
screens, dialogs, stream chrome) and `Layer.svelte` (the sky, pointer parallax, cover halos, a small forecast line).

- **Sky**: four soft gradient blobs and two swaying "curtains" plus a few stars, all plain gradients (no blur filters, so it
  stays cheap). Not mounted on `/stream`. Static under `prefers-reduced-motion`.
- **Parallax**: `Layer` eases the pointer into `--px` / `--py` (-1..1) on `<html>` with one rAF loop that sleeps when settled.
  Cards read them with `translate3d` + small `rotateX/Y`; the blobs move the other way. Off under reduced motion.
- **Spotlight**: `--mx` / `--my` drive a soft light that follows the pointer across cards and dialogs.
- **Dock**: the sidebar is a floating pill at the top. Icons open into labelled pills on hover or focus (the current page
  stays open), neighbours lean in. "Coming" items stay in the dock, dim with a dashed ring. The host pill and its menu sit at
  the right end.
- **Type**: Outfit variable (light) for UI, Instrument Serif italic for page titles, hero and game names, the Ctrl+K input.
- **Cover halos**: on first hover or focus of a game card (and at once on the Library hero) `Layer` puts a blurred copy of the
  cover's own art behind it, so the glow is the cover's colours.
- **Forecast line**: after each page change a pill says things like "Aurora forecast: Kp 6, skies clear, lag low" for 4 s.
- **Accent**: `var(--accent)` tints one aurora blob and one curtain, focus rings, the active dock item, selected chips and
  tabs, buttons and glows. `--accent-fg` follows the accent too.
- **Motion**: long springs (`--spring-settle`) for the dock and halos, snappy for buttons and magnification.

## Fonts

- `@fontsource-variable/outfit` 5.3.0 and `@fontsource/instrument-serif` 5.3.0, imported in `theme.css`.

## Needs shared change

Nothing.

## Known limits

- Halos are created lazily on hover, so a card that was never hovered has none. They sit inside the row, which pads itself to
  leave room.
- Backdrop blur is on every card, so a very full Home on a weak GPU can drop frames.
- The Settings tab pill follows `.ind`, which only moves when the tab strip is not scrolled.
