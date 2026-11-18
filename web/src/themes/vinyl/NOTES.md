# Record shop (vinyl)

Kraft paper page, a walnut side wall with a shop sign, games as square record sleeves standing in wooden crates.

- Hover or focus a sleeve: it lifts, a black disc slides out of the right side and spins. Price-sticker tags on sleeves.
- Nav items are alphabet divider cards (A, B, C ...), the active one is pulled out of the crate. "Coming" items are blank cards.
- Home Resume card is a turntable bar: the cover is the spinning record, the tonearm swings in on load.
- Library hero and the game detail head show a disc peeking out of the sleeve.
- Layer.svelte: a small turntable in the side wall (hidden below 760 px height, on /stream, and while the host menu is open).
- Host pill is a price tag. Toasts are label slips. Command menu is a paper card.
- Fonts: DM Serif Display, Bowlby One, Special Elite, IBM Plex Mono, Familjen Grotesk (all already in package.json).
- Motion: disc spin, sleeve lift, tonearm drop, sign swing, entrance pull. All stop under prefers-reduced-motion.

## Accent
Used for the active divider card, primary buttons, pressed tabs and chips, the disc label colour and counts.
Accent values are darkened per hue so white text and the accent as text pass AA on cream.

## Needs shared change
Nothing required. Nice to have: Library cards could carry a data attribute for the saved quality so the sticker could read
"HOT 62 ms"; today the sticker styles the existing chips only.

## Not checked
GhostBar was checked only in the failed-connection state with the bar forced visible (no live WebTransport in the mock).
