# Swiss editorial

Paper ground, black ink, one red accent (`#d40010`, forced over the accent setting). Square corners, no shadows, no cards:
content sits between rules. The sidebar is a numbered text index. Page titles are 11vw bold grotesque.

## Files

- `theme.css`: everything, all under `:root[data-ui='swiss']`.
- `Layer.svelte`: draws the 12-column hairline grid behind the page (not on /stream) and sets `html[data-swiss-page]`
  so the CSS can print the section kicker ("02 / Games") and the Home title (`main::before`, Home has no heading).

## Fonts

- `@fontsource-variable/familjen-grotesk@5.3.0` (text and titles)
- `@fontsource/ibm-plex-mono@5.3.0` (numbers and metadata, weights 400 and 500)

## Motion

Blocks wipe in left to right (clip-path, `swiss-wipe`), the rule under each page title draws, a red line draws under a
nav item on hover. Dialogs fade in 90 ms with no movement. All of it sits under `prefers-reduced-motion: no-preference`.

## Needs shared change

Nothing. Known limits: the Home gauges are flat bars instead of rings (CSS only); the Stream idle page gets its own
title through `.stream::before`, since /stream covers the sidebar.
