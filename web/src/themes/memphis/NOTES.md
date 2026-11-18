# Memphis

Memphis Group meets Saved by the Bell: cream paper with dots, squiggles and confetti, hard offset shadows, leaf-shaped cards
that sit at slight angles and bounce on hover. Light only. Fonts: Rubik Mono One (titles, labels) and Rubik (everything else).
Files: `theme.css` (tokens, rail, screens, dialogs, stream chrome) and `Layer.svelte`.

- **Rail**: a dotted purple sidebar with a zigzag edge. Nav items are cream pills tilted left and right; hover turns them
  yellow, teal or pink, the current page is pink with a yellow arrow. "Coming" items keep a dashed outline. The host pill is
  teal at the bottom, its menu is a cream leaf.
- **Cards**: 3px ink border, a hard shadow that cycles pink, teal, yellow, purple, a coloured corner blob and a dot texture.
  Hover straightens the card and lifts it with a springy overshoot. Card labels (h4) are coloured pills.
- **Titles**: page titles are Rubik Mono One with a pink zigzag underline. Tabs sit on the same zigzag.
- **Covers**: leaf-cornered with a coloured hard shadow, tilted a little, lift on hover.
- **Background**: the root carries a confetti tile and a dot grid. `Layer.svelte` adds 14 drifting shapes (triangles, rings,
  dots, squiggles, plus signs) behind the page. They are `pointer-events: none` and not mounted on `/stream`.
- **Delights**: clicking a `.btn` or a Play button throws confetti; after each page change a small yellow sticker says
  something short ("Radical.") for 3 s.
- **Accent**: drives primary buttons, selected chips and tabs, focus rings. Values are dark enough for white text.
- **Motion**: cards pop in with an overshoot, pages slide in with a small bounce. Reduced motion: no drift, no confetti, no
  sticker animation, near-zero transitions.

Known gaps
- The live stream chrome (bar, menus) is restyled but was only checked in the idle and failed states.
- `label-content-name-mismatch` axe hits on game covers also appear in every other style with the mocked axe run; not from this style.
- Rubik Mono One is wide: very long game names in the hero or detail header wrap by letter (`overflow-wrap: anywhere`).
