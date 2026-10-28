# Stream screenshots (roadmap Stage 6b)

An experiment on `exp/stream-screenshots`. A screenshot of an emulator game is a frame grab from the stream
in the browser, kept on the machine that runs the game.

## What it does

- The Screenshot button in the stream bar and Ctrl+Alt+S already existed: they grab the stream canvas to a PNG,
  download it and copy it. Nothing new on the keyboard. Ctrl+Alt+S stays reserved and never reaches the game.
  A configurable key was not added.
- For an emulator game whose hostd says `caps.screenshots`, the same PNG is also posted to the machine
  streaming it. The toast then shows a small thumbnail and says it was kept with the game. The bar tip says so too.
- Steam games keep Steam's own screenshot folder. They are not uploaded. An old hostd sends no caps, so its
  emulator games read as `screenshots: false` and nothing is uploaded.
- One capture at a time: a second press says "Still saving the last screenshot".
- The game panel's "Your screenshots" grid shows the stored shots for emulator games, with a Delete button on
  each tile. Delete asks first (Delete / Keep).

## hostd routes (all need the secret)

- `POST /api/games/{id}/screenshots`: raw PNG body. 201 with `{name, width, height, size, takenAt}`.
  422 for a bad PNG, 413 over 8 MiB (counted while it streams), 507 when the disk is short, 404 for Steam
  or unknown games.
- `GET /api/games/{id}/screenshots`: the list, newest first.
- `GET /api/games/{id}/screenshots/{name}`: the PNG (`nosniff`, private cache). Same route as Steam's; emulator games read from the store.
- `DELETE /api/games/{id}/screenshots/{name}`: 204, also when it is already gone.
- `GET /api/games/{id}/local` lists the names for emulator games, so the panel needed no new fetch.
- `caps.screenshots` is now true for emulator games. It was the existing "has screenshots" flag, false for them by design.

## Limits and checks

- Files: `data_dir/shots/<id with : as _>/<unix ms>.png`, mode 0600, directories 0700, written to a temp file and renamed.
- 8 MiB per image, 200 per game (the oldest are dropped), width and height at most 8192, 256 MiB of free space kept.
- A PNG must have the signature, a first IHDR chunk with sane size, and end with IEND. It is not decoded.
- Names are only `<10 to 16 digits>.png`. Anything else is a 404.
- The hub proxy needs no change: the body is under its 16 MiB cap.

## Capture notes

The stream draws WebCodecs frames to a canvas from our own origin, so the canvas is not tainted and
`toBlob` works. If a browser ever refuses to read it, the toast says so. The PNG is at the stream's own
resolution (1080p is about 2 to 4 MiB; a very busy 4K frame could pass 8 MiB and is refused with a message).
Encoding runs in the browser's `toBlob` and is not timed here.

## Not verified

- Not run against a real stream, a real Dolphin game or through a real hub. Only unit, route and component tests.
- Large frames at 1440p and above were not measured against the 8 MiB limit.
- The thumbnail toast and Delete confirm were not looked at by eye.
- The proxy test uses a 1 MiB body (the mock upstream limits bodies to 2 MiB); the 8 MiB case rests on the cap arithmetic.
