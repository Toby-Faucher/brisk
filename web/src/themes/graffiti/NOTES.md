# Graffiti

A street wall at night. The page is a dark brick wall (an SVG tile under a dark wash, plus film grain and two soft colour
stains from `Layer.svelte`). Page titles are spray tags in Permanent Marker with paint drips. Cards are dark slabs held up
with a strip of tape and a spray-colour underline. Library covers are wheat-paste posters: paper border, torn corners,
tape, a drop shadow, a small tilt that straightens on hover. The sidebar becomes a left rail of die-cut stickers (white
edge, tilted, one neon colour each); the current page is the one with a peeling corner and a spray ring. The host pill is
a paper label at the bottom of the rail. Buttons are stencil slabs with hard shadows that press down.

`Layer.svelte` (not on /stream): wall grain and stains, a big outlined BRISK tag, a stencil sign in the rail with one line
per page, and a spray-can trail. The trail is dots that follow the pointer in five neon colours and fade in under a second.
One canvas, one rAF loop that sleeps when no dots are left. Off for touch and `prefers-reduced-motion`.

Fonts (self-hosted, exact pins, imported in theme.css):
- `@fontsource/permanent-marker` (tags, shelf titles, game names, tabs)
- `@fontsource/rubik` 400, 600, 800 (body, labels, stencil caps)

Accent: not overridden, so the Settings swatches work as is (Resume, Play and Start buttons take it).

Needs shared change: none.

## Checked
1440x900 screenshots of Home, Library, Machines, History, History session, Settings (3 tabs), Stream idle, Ctrl+K menu,
quit dialog, game detail. Axe on every view of `scripts/axe-check.mjs` (run against a copy with `/api/machines` answering
404 and the style preset): no colour-contrast or other findings from this style.

## Known gaps
- `label-content-name-mismatch` on game cards shows up in the axe run for the original look and for sticker too, so it is
  not from this style.
- Axe cannot measure text that sits straight on the brick (background image). The wash keeps it near 12:1 by eye and by
  the colours used (#f4f0e3 and #cdc7b6 on about #1a1214).
- The live stream controls were only checked in the degraded state (no WebTransport host here).
- Under 760px wide the rail stays 146px wide; there is no phone layout for it.
