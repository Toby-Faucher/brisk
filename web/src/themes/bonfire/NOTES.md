# Bonfire (bonfire)

A bleak dark-fantasy action RPG menu. Ash and bone text on near-black, one ember-orange accent, IM Fell English and Cinzel.

- Narrow left column with plain serif entries. A thin ember marker and a small flickering glow sit on the active item.
- Cards, dialogs and the Ctrl+K menu are worn inventory slots (dark inset frames). Covers are desaturated until hovered.
- Home gauges are thin health bars. Buttons are small engraved plates that warm up on hover.
- `Layer.svelte`: drifting embers, slow fog, a vignette, a thin health and stamina strip, and a fading banner on page
  change ("Rest awhile." and similar). All `aria-hidden`, `pointer-events: none`, not mounted on /stream.
- Accent: the default keeps ember orange; other accents tint the ember.
- Reduced motion: no embers, fog drift, flicker or banner.

Fonts: `@fontsource/im-fell-english`, `@fontsource/cinzel`.

## Known gaps

- IM Fell uses old-style numerals, so figures look uneven.
- The banner is hidden under reduced motion rather than shown still.
- `.note` and `.link` get a solid dark background so axe can read the contrast.
