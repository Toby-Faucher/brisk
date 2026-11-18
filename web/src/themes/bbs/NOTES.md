# BBS (dial-up board)

A 1990s bulletin board in 16-colour ANSI. Black screen, a blue status bar top and bottom, double-line frames drawn with
`border: 3px double`, IBM Plex Mono everywhere. No scanlines, no glow: it is about colour and boxes.

- Fonts: `@fontsource/ibm-plex-mono@5.3.0` (400 and 700), self-hosted.
- Layout: the sidebar is a framed "MAIN MENU" with lettered entries ([H] Home, [L] Library ...). The picked page is a cyan
  lightbar. Main is one big double frame whose title is cut into the top edge (FILE LIBRARY, NODE LIST, CALLER LOG ...).
  "Coming" entries become locked doors. The host pill becomes an "ONLINE:" box. The machine-off banner says NO CARRIER.
- `Layer.svelte`: top bar with a typed modem handshake (static under reduced motion), a fake caller counter (kept in
  localStorage, +1 per page load) and a clock; bottom bar with the real key hints, a "time left" joke that counts down
  while the tab is open, and a blinking block cursor. It also holds the SVG filter that posterizes covers. Nothing
  renders on /stream.
- Covers are posterized to 4 levels per channel with a dot dither on top. Page titles end in a blinking block cursor.
- Accent: the picked accent is the lightbar, primary buttons and cursor. The ANSI colours stay fixed.
- Narrow screens (under 760px): the menu becomes a strip above the page. Home's own card grid is not responsive in any style.

## Known gaps

- The letters in [H] [L] ... are labels, not keyboard shortcuts.
- The letter-per-item map is by position (Home, Library, Stream, Machines, History, Settings). A new module shows [?].
- axe on the library fails `label-content-name-mismatch` for game cards in every style, including the original look.
  `scripts/axe-check.mjs` also has a stale mock (`/api/machines`) and a stale "Open Portal 2" label.

## Needs shared change

Nothing.
