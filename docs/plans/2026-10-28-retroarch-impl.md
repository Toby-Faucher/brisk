# RetroArch support (experiment)

Branch `exp/retroarch`, draft PR to main. Research: `docs/research/retroarch-notes.md`. Nothing here was installed, run
or deployed on the games VM or the hub; every VM-dependent claim is in "Unverified" below.

## What was built

- **Registry** (`registry.rs`): a second row, `retroarch`, with six systems. `System` gained `core`, the core file name. Fixed core per
  system: NES Mesen (Nestopia is the alternative), SNES Snes9x, Mega Drive Genesis Plus GX, GB and GBC Gambatte, GBA mGBA.
  Folders and ids follow `ingest.rs`: `nes snes megadrive gb gbc gba`.
- **Launch** (`retroarch.rs`, `emu.rs`): argv is `retroarch -L <cores dir>/<core> -f <file>`; Dolphin's argv is unchanged. Cores dir and
  per-system core names come from env (`docs/env.md`). A missing core file or a record with no system logs a clear error and the launch
  fails. The file is its own argv entry.
- **Running and quit**: process name `retroarch` and the file in argv, as for Dolphin, so running, playtime and last played are reused as is.
  The window match (`running.rs`) accepts X11 class `retroarch` or a Wayland app id equal to it or ending in `.retroarch` (any case), so
  `com.libretro.RetroArch` matches. Quit is the existing path: sway close request, wait, SIGTERM; no pad quit hotkey.
- **Library**: scans pick up `nes smc sfc swc fig md smd gen gb gbc gba` (not `.bin`, not `.zip`). Cartridges used to count as `unplayable`;
  now that an emulator plays them they register as `emu:retroarch:<16 hex of SHA-1>` (the existing `discid.rs` id). Tests use synthetic files only.
- **Saves** (`saves.rs`): `GET /api/games/{id}/saves` lists `saves/<stem>.srm` and `.rtc` (kind `sram`) and `states/<stem>.state`, `.stateN`,
  `.state.auto` (kind `state`) under `BRISK_RETROARCH_DIR`. Names, sizes, times only.
- **Art** by name through the existing path (no change). **Achievements**: the route answers `{"state":"unsupported","reason":...}` for
  RetroArch games; `caps.achievements` stays false. Hashing these consoles (MD5 rules in the notes) is a later job; the RetroArch login is manual.
- **Setup** (not run): `scripts/setup-retroarch.sh` with `deploy/retroarch/`: `retroarch.cfg.fragment` (fullscreen, `confirm_quit` off,
  `pause_nonactive` off, `video_driver`, `audio_driver`, autosave, auto states, `system_directory` to an empty folder, saves/states
  folders) and `Sunshine_X-Box_Series.cfg`, a joypad profile for the Sunshine pad. Adds absent keys, `--force` to replace.
  Test: `scripts/test-setup-retroarch.sh` (temp dir only).
- Hardcore is left off (RetroArch default for brisk: nothing set). No BIOS.

## Unverified (needs the VM)

1. Window id: native Wayland app id or Xwayland class. Both match; confirm which, and that focus and fullscreen work.
2. SIGTERM: does RetroArch exit and flush its `.srm`? If not, the sway close request is what ends it (confirm that works too).
3. `confirm_quit = "false"` ends the prompt (key name read in the source; behaviour not run).
4. `.state.auto` file name and that autosave writes where the saves list looks.
5. `video_driver = "gl"` and `audio_driver = "pulse"` on the VM; the Sunshine pad autoconfig (match by name; button indices by analogy).
6. Library ids for cartridges; art matches by name for cartridge titles.
7. Two games with the same file stem on different systems share save names (known shortcut in `saves.rs`).

## VM install and first test (when Toby is not playing)

Install through the queue (root): `sudoq run --why "RetroArch and cores for brisk" -- pacman -S --needed retroarch libretro-mesen libretro-snes9x libretro-genesis-plus-gx libretro-gambatte libretro-mgba libretro-core-info`
(add `libretro-nestopia` only if Nestopia is wanted). Then as the games user:

1. `pacman -Ql libretro-mesen libretro-snes9x libretro-genesis-plus-gx libretro-gambatte libretro-mgba | grep '\.so'`: the file names must match the registry (the Arch file lists say they do).
2. `scripts/setup-retroarch.sh` (RetroArch closed), read its output, then `diff` the fragment keys against `~/.config/retroarch/retroarch.cfg`.
3. Redeploy hostd with this branch, scan the games folder with a few real cartridges, check they appear and Play starts them.
4. During a game: `swaymsg -t get_tree | grep -E 'app_id|class'` for the window id; check it is fullscreen and focused.
5. Make an in-game save, then end the game from brisk: the stream's quit, then `kill -TERM` of the process by hand on a second run. Look at the `.srm` time.
6. Check no quit prompt appears on a close request (`confirm_quit`).
7. Play a minute, close, reopen: autosave state resumed; `GET /api/games/<id>/saves` lists the `.srm` and `.state.auto`.
8. Pad: d-pad, face buttons (Xbox A is RetroPad B), triggers; if wrong, fix `deploy/retroarch/Sunshine_X-Box_Series.cfg`.
9. Only then: RetroAchievements login inside RetroArch's menu (manual).

## Web changes needed (no UI built; each needs a mock-up first)

- System badges and labels for `nes snes megadrive gb gbc gba` (today only `wii` and `gamecube` are known).
- Saves: `web/src/state/api.ts` kind union gets `sram`; `library.ts` `saveRow` labels it ("Battery save") and picks an icon. Until then an
  `.srm` row would read "Wii save".
- Box art aspect ratios for cartridges (square-ish or 4:3, not disc cases).
- An Emulators tab section for RetroArch (cores per system, core override, install state). The existing controls card stays hidden: `controller` is empty.
- Achievements panel: `unsupported` with a reason is already a generic state.
