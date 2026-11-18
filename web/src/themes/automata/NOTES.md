# Android log

Khaki paper, dark brown text boxes, hairline frames. Files: `theme.css` and `Layer.svelte` (dot grid, scan line, wipe caption).

- **Menu**: a narrow list of dark boxes with a small square bullet. Hover shifts a box and inverts it to paper. The current page is
  a boxed row with a blinking accent square. Under 760px it becomes a strip across the top.
- **Page change**: a thin paper bar wipes down the page (the menu stays) and a small caption ("READING LIBRARY") blinks. Skipped
  under reduced motion and on `/stream`.
- **Panels**: dark boxes with a hairline frame and a faint outer rule. Page text on paper is dark; text in boxes is light.
- **Type**: Share Tech Mono (headings, menu), IBM Plex Mono (text). One muted rust accent.

Known gaps: Library covers and History rows could not be viewed here (the mock `/api` never leaves the loading state in preview).
Home on a phone keeps the base six-column grid, so cards are tight.
