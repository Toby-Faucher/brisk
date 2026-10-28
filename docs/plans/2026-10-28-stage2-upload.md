# Stage 2 backend: games folder, scan, resumable upload, remove, add

Status: built in hostd on branch `stage2-ingest`. Context: `docs/plans/2026-10-28-emulator-roadmap.md` (Stage 2).
Header reading: `docs/research/discid-notes.md`. The web half and the hub half are separate (see the end).

## Three ways in

1. Upload from the Library to the chosen machine (resumable, below).
2. Scan: files put in the games folder any way (scp, a share) are found and get cards.
3. A script: `POST /api/uploads` from a remote script, or `brisk-hostd add <file>` on the machine.

## The games folder

`~/Games` per machine, `BRISK_GAMES_DIR` overrides. One sub-folder per system, chosen from the file header and
never from the file name: `wii`, `gamecube`, `nes`, `snes`, `megadrive`, `gb`, `gbc`, `gba`, `unknown`.
`.uploads/` inside it holds the partial files.

The games VM keeps working as it is: its `library.json` records hold absolute paths, so `~/roms` needs no change.
Toby may link it as `~/Games/wii` later. A scan follows a symlink only when it is a direct child of the games
folder (so that link works); symlinks deeper down and symlinked files are never followed.

## Ids and aliases

- Wii and GameCube: `emu:dolphin:<GAMEID>`, the 6-character disc ID from the header (`emu:dolphin:RMCE01`).
- Cartridge systems: `emu:<emulator>:<first 16 hex of the SHA-1>` (SHA-1 of the file without a copier header).
  This is a plain SHA-1, not the RetroAchievements hash. No cartridge emulator is in the registry yet, so this
  rule is built and tested but no cartridge record is written today.
- Dedupe: a file whose canonical path is already in the library, or whose id (or alias) is already there, is
  not added twice. If the path is known under another id (an older slug id), that record keeps its id and gains
  the new id as an alias; `GET /api/games` answers the alias with the record's id.
- Two different dumps of one disc ID: the second is counted as a duplicate and not listed.

## Systems without an emulator

Only Wii and GameCube (Dolphin) are playable. A cartridge file is still identified and, on upload, stored in its
system folder, but no library record is written (a record needs an emulator). Scan counts them as `unplayable`.
When an emulator is added to `registry.rs` for that system, the next scan registers them. `unknown` holds files
with a known extension that the header does not confirm (upload only); they are stored, not registered.

## Library file

`library.json` is read, changed and replaced under an exclusive `flock` on `data_dir/library.lock`, so a scan, an
upload finish and `brisk-hostd add` cannot lose each other's change. The write is atomic (temp file, rename). A
`library.json` that does not parse is never overwritten: the call fails with 500 and the file stays for repair.

## Scan

`POST /api/library/scan`, no body. Walks the games folder: depth 3, at most 20000 entries, regular files only,
skips names starting with `.` (so `.uploads`), `*.part`, and anything whose extension is not one of
`iso gcm wbfs rvz wia nes sfc smc gb gbc gba md gen`. Reads the header with `discid::identify`; a Wii/GC disc
reads a few KB, a cartridge is hashed (streamed, whole file). One scan at a time (409 while one runs) and at
least 5 seconds between scans (429 with `retryAfterS`). Answer:
`{scanned, added: [Game], aliased, duplicates, unplayable, unknown, broken, truncated}`. `broken` counts known
formats with a damaged header. `BRISK_SCAN_ON_START=1` runs one scan at startup.
A game that was removed comes back at the next scan (the file is still there). Keep the file out of the
games folder, or move it, if it must stay gone.

## Resumable upload

All routes sit behind the usual auth (`x-brisk-secret`, or an allowed tailnet login). Errors are
`{"error": "..."}`.

`POST /api/uploads` body `{"name": "Game.iso", "size": 4699979776}` -> 201 `{id, chunk, offset: 0}`.
- `name`: no `/`, `\` or NUL; passed through `files::sanitize`; its extension must be in the list above (415).
- `size`: at least 1. No other limit than free space: refused with 507 when `free - size < 5 GB`.
- At most 2 uploads open (429). `id` is 128 random bits as hex. `chunk` is 33554432 (32 MiB), the largest body.

`PUT /api/uploads/{id}` header `x-offset: <n>`, body = the next bytes -> 200 `{offset}` (the new offset).
- `x-offset` must equal the current offset, else 409 `{error, offset}` with the current one. A missing or
  bad header is 400.
- Body 1 byte to 32 MiB (Content-Length over it is 413; a stream over it is cut off with 413). It must not run
  past `size` (413). Written frame by frame, never buffered.
- A chunk counts only when its whole body arrived. If the connection drops, the offset does not move and the
  half-written bytes are cut off at the next PUT. One PUT at a time per upload (409 `busy`).
- Free space is checked before every chunk, so at most 32 MiB apart (tighter than a check every 64 MiB, and
  a chunk is smaller than 64 MiB, so a check inside a chunk would never fire): `free - remaining < 5 GB`
  -> 507. The upload stays open.

`GET /api/uploads/{id}` -> `{id, name, offset, size, state}`; `state` is `open` or `busy` (a chunk is arriving).
A client that lost its connection calls this and carries on from `offset`. 404 for an unknown id.

`POST /api/uploads/{id}/finish` -> 200 `{system, registered, duplicate, game}`.
- `offset` must equal `size`, else 409 `{error, offset, size}` and the upload stays open.
- Runs `discid::identify` on the `.part`. A damaged header is 422 and the upload is removed. System unknown with
  a known extension goes to `unknown/`, not registered.
- Moves the file with a hard link then unlinks the `.part` (never overwrites; on a name clash the name gets
  ` (1)`, ` (2)`) to `<games>/<system>/<sanitized name>`, then registers it as a scan would (`source: "upload"`).
- `game` is the new card, or the existing one when `duplicate` is true (the file is kept anyway). `game` is null
  when `registered` is false (no emulator for that system).

`DELETE /api/uploads/{id}` -> 204, removes the `.part`. Idempotent.

Files: `<games>/.uploads/<id>.part`, mode 0600, directory 0700. At startup `.part` files older than a day are
removed. Upload state is in memory: a hostd restart forgets open uploads (the client gets 404 and starts again);
their `.part` goes at the next startup after a day. An open upload idle for a day is dropped when the next one
starts.

## Remove

`DELETE /api/games/{id}` -> 204. Emulator games only; `steam:*` is 400 `not supported`. It forgets the library
record. The file stays on disk. Notes, tags, prefs and playtime stay under the same id, so a re-add (a scan, the
same disc ID) brings them back. An unknown or already removed id is 204 too. A malformed id is 400.

## Script path on the machine

`brisk-hostd add <file> [--move|--copy|--link]`. It identifies the file and writes the library record without
the server. With no flag the record points at the file where it is. `--move`, `--copy` and `--link` first put it
at `<games>/<system>/<name>` (never overwriting; `--move` renames, or copies then deletes across disks; `--link`
makes a symlink, which a later scan ignores, so the record is its only registration). Exit 0 added or already
known, 1 failed (reason on stderr), 2 usage. A system with no emulator is refused. Remote scripts use the upload
API; the curl recipe is in `docs/hosting-runbook.md`.

## Hub change needed (not done here)

Today `machines::proxy` buffers the body up to `CAP` (16 MiB), refuses any path whose first segment is `files`,
forwards only `content-type` and `accept`, and times out after 40 s. Chunks make a streaming proxy unnecessary:
each PUT is at most 32 MiB, so a bounded buffer is enough. Exact change in `proxy`:

1. Path `uploads` or `uploads/*`: use a body cap of `33 << 20` (chunk plus slack) instead of `CAP`; keep `CAP` for
   everything else. Memory is bounded by 2 uploads x 33 MiB.
2. For those paths use a 300 s timeout instead of `Duration::from_secs(40)`.
3. Forward the request header `x-offset` (add it beside `content-type` and `accept` and pass it to `call`).
4. Forward the response unchanged: status (409 and 507 included), body, content-type. No new response header.
5. Tests: a PUT of 20 MiB through the hub reaches the machine; 34 MiB is 413; `x-offset` arrives; `files/x` is
   still 501.

The browser's hub session must send chunks one at a time per upload (the machine refuses a second).

## What the web half builds

- Library "Add game" button: pick the machine (default: the one shown), pick files.
- Per file: `POST /api/uploads`, then loop `PUT` of 8 to 32 MiB slices with `x-offset`; on a network error or 409,
  `GET` the upload and continue from its `offset`; on 507 show "not enough free space (5 GB is kept free)"; on
  415 "not a game file"; at the end `POST .../finish` and show the card (or the `registered: false` note:
  "stored, but no emulator for this system yet").
- Progress from `offset / size`; cancel = `DELETE`. Two files at a time at most.
- "Remove" in the game detail menu -> `DELETE /api/games/{id}`, with the text that the file is kept.
- A "Scan games folder" button -> `POST /api/library/scan`, then refresh the list.
- Through the hub (`/api/m/{id}/uploads...`) only after the hub change above.
