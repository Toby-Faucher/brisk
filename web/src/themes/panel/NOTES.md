# Hardware panel

A warm grey enclosure with one signal orange. The sidebar becomes a deck along the top: hard keys that press down
(translateY plus a shadow that collapses), a small LCD with CPU and GPU LED bars (`Layer.svelte`, reads the `host`
store, hidden on /stream and under 1180px), and the host pill as a dark readout window. Cards are faceplates with
screws; checkboxes are slide switches; chips and Settings tabs are latching keys with a pilot light; progress bars are
LED segments. Motion is stepped (the page powers on with a short flicker, panels slide up in steps, dialogs latch in
with a small overshoot).

## Fonts

- `@fontsource/space-grotesk` 5.3.0 (body and headings)
- `@fontsource/ibm-plex-mono` 5.3.0 (printed legends, readouts)

Both are imported at the top of `theme.css`.

## Notes

- The accent is fixed to orange. The Accent swatches in Settings still work but have no visible effect in this style.
- The stream chrome (GhostBar, stats card) is a dark LCD so it stays readable over video.
- `label-content-name-mismatch` on game cards also fails in the original look (axe-check); not caused by this style.

## Needs shared change

Nothing.
