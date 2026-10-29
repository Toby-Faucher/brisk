# RetroArch for brisk: research notes

Date: 2026-10-28. For the roadmap stage "more emulators" (`docs/plans/2026-10-28-emulator-roadmap.md`).
Sources: docs.libretro.com, the libretro/RetroArch repo (raw files on master), the Arch package pages
(archlinux.org/packages), docs.retroachievements.org. Nothing here was run: RetroArch is not installed on the
machine where this was written. Claims I could not confirm from a primary page are marked **unverified**.
The Arch wiki page for RetroArch returned an Anubis "Access Denied" page, so it was not read. The WebFetch tool
summarises pages with a small model, so exact strings below are as it reported them.

## Update after the implementation (2026-10-28)

Checked against primary sources without installing anything: raw files of libretro/RetroArch (master),
libretro/retroarch-joypad-autoconfig, and the Arch package file lists (`archlinux.org/packages/extra/x86_64/<name>/files/json/`).
Newer facts win over the older text below.

- **Core file names are now verified** from the Arch file lists: `usr/lib/libretro/{mesen,nestopia,snes9x,genesis_plus_gx,gambatte,mgba}_libretro.so`
  (each package holds that one `.so` plus licence files). `retroarch` installs `usr/bin/retroarch`, `etc/retroarch.cfg` and
  `usr/share/applications/com.libretro.RetroArch.desktop`, so the Wayland app id is very likely `com.libretro.RetroArch`; still read it on the VM.
- **Confirm quit key is `confirm_quit`** (bool, `DEFAULT_CONFIRM_QUIT true` in `config.def.h`; `configuration.c` migrates the old
  `quit_press_twice` to it). The text below that guesses `confirm_on_exit` is wrong.
- **Autoconfig keys verified** against `udev/Generic_X-Box_pad.cfg` in the autoconfig repo: `input_device`, `input_driver`,
  `input_{a,b,x,y,l,r,select,start,l3,r3}_btn`, `input_{up,down,left,right}_btn = "h0up"` (hat), `input_{l2,r2}_axis = "+2"/"+5"`,
  `input_{l,r}_{x,y}_{plus,minus}_axis`. `deploy/retroarch/Sunshine_X-Box_Series.cfg` is that profile with the Sunshine device name,
  indices taken from the Dolphin mapping (axes 0/1 left stick, 3/4 right stick, 2/5 triggers, hat 6/7). Match is by device name;
  the pad's vendor and product ids were not read. Not tried on a pad.
- `retroarch.cfg` sample confirms `video_fullscreen`, `pause_nonactive` (default true; brisk sets false), `autosave_interval`
  (Linux default 0 in `config.def.h` by platform `#if`, so set it), `savestate_auto_save` / `savestate_auto_load` (both default false in
  `config.def.h`), `config_save_on_exit`, `system_directory`, `savefile_directory`, `savestate_directory`, `joypad_autoconfig_dir`.
- Still unverified: SIGTERM behaviour, native Wayland vs Xwayland, the `.state.auto` name, which `video_driver` / `audio_driver` values
  work on the VM (`gl`, `pulse` are guesses), and that the autoconfig name match is enough.
- What brisk does with this: `docs/plans/2026-10-28-retroarch-impl.md`.

## Short answer

- RetroArch is one frontend, many cores. It needs one core per system, so the registry row needs a core path
  chosen per system (today a row has one fixed `args` list).
- Arch ships `retroarch` and the cores below as official `extra` packages. The best NES core by
  RetroAchievements' own list (FCEUmm) is not in the official repos; Nestopia and Mesen are.
- Launch is `retroarch -L <core>.so -f <file>`. Quit cleanly with the quit hotkey or a polite signal
  (**unverified**), never SIGKILL.
- Controllers: the Sunshine pad is a plain evdev gamepad, so RetroArch's udev driver sees it. Autoconfig files
  are not shipped by the Arch package, so they must be fetched once or written by brisk.
- Achievements: RetroArch logs in once in its own menu, then needs the right core and hardcore on. The hash is
  MD5 for SNES, Mega Drive, GB, GBC, GBA, N64; NES and PSX are custom. `discid.rs` is not in this tree; the
  roadmap only plans a SHA-1 fallback id, and that is a different job (see section 5).

## 1. Launching one game

Verified from docs.libretro.com (CLI page) and `retroarch_print_help()` in `retroarch.c`:

| Option | Meaning |
|---|---|
| `-L <core>` | Load this libretro core (full path to the `.so`). Example on the docs page: `retroarch -L /path/to/libretro/core.so game.rom`. |
| `-f`, `--fullscreen` | "Start the program in fullscreen regardless of config setting." |
| `-v`, `--verbose` | "Verbose logging." (The docs page names `--verbose`.) |
| `--appendconfig <file>` | Extra config files are loaded on top of the main one and take priority. Their settings are written to the main config whenever it is saved, on exit included. |
| `--config <file>` | Use this file instead of the default `retroarch.cfg`. |
| `--menu` | Start in the menu with no content or core. With content on the same line it is an error: the code logs "--menu was used, but content file was passed as well." and exits. So `--menu` is not a way to launch a game; it is only for the bare launcher. |
| `-s`, `-S` | Save and state paths; both are marked DEPRECATED. Use `--appendconfig` plus `savefile_directory` / `savestate_directory`. |

Also: `--log-file`, `--max-frames`, `--subsystem` exist as option ids in the source (`RA_OPT_LOG_FILE` and so on);
their help text was not read. **Unverified** beyond the names.

Suggested argv for one game: `retroarch -L /usr/lib/libretro/snes9x_libretro.so -f --appendconfig <brisk.cfg> <file>`.
Pass the file as its own argv entry, like Dolphin today, so no shell parses the path.

Config file locations (CLI docs page): `$XDG_CONFIG_HOME/retroarch/retroarch.cfg`, else
`~/.config/retroarch/retroarch.cfg`. The Arch package also installs a template at `/etc/retroarch.cfg`
(Arch file list). Defaults come from `config.def.h` and `retroarch.cfg` in the repo.

Because `--appendconfig` settings are written back to the main config on exit, a brisk-owned append file works,
but its values will leak into `retroarch.cfg` over time. That is acceptable if brisk always passes them.

### Under sway on Wayland

- RetroArch has a native Wayland context (`wayland_ctx.c`, driver name "wayland"). It sets the xdg app id from
  `#define WAYLAND_APP_ID "com.libretro.RetroArch"` (`wayland_common.h`), for both the xdg-shell and libdecor paths.
- When it runs through X11 or Xwayland, `x11_common.c` sets the class hint to name `retroarch`, class `retroarch`.
- So the window is either app_id `com.libretro.RetroArch` (native Wayland) or X11 class `retroarch` (Xwayland).
  Which one a given Arch build and config picks depends on `video_context_driver` and the SDL/GL setup. **Unverified**; the
  first run on the games VM must read `swaymsg -t get_tree` and settle it. The registry's `window_class` is one
  string and the focus code reads the X11 class, so a native Wayland window (which sway shows under `app_id`) would
  not match until `focus` also tries `app_id`. Simplest fix: force Xwayland with an env var or the config
  (`video_context_driver = "x"` is my guess at the key, **unverified**) or teach focus to accept either.
- Arch's `retroarch` needs `wayland`, `libx11` and `sdl2-compat` at runtime and suggests `libdecor`. Under a kiosk
  sway there is no decoration need, so `libdecor` is optional.

### Fullscreen

`-f` is enough. Config also has `video_fullscreen` (default false in `retroarch.cfg`) and
`video_windowed_fullscreen` (default true). Brisk's existing focus loop also sets fullscreen via sway, which
would work either way.

### Exiting cleanly

- Quit hotkey: `input_exit_emulator`, default Escape. `retroarch.cfg` says it exits cleanly and that "Killing it
  in any hard way (SIGKILL, etc) will terminate RetroArch without saving RAM, etc."
- Pad quit: `input_quit_gamepad_combo` (default 0, off). Values are a combo style number; the exact list is
  **unverified**. Hotkeys from the Sunshine pad need "Enable Hotkeys" bound to a button to avoid accidents.
- Confirm on quit: the source has `DEFAULT_CONFIRM_QUIT true` in `config.def.h`, so there is a confirm-quit
  setting and it is on by default. The config key name was not found in the part of `configuration.c` I could
  read; I believe it is `confirm_on_exit` but that is **unverified**. This is the counterpart of Dolphin's
  ConfirmStop. For a kiosk where brisk ends the game, turn it off in the append file, otherwise the first
  quit press shows a prompt and nothing exits.
- Signals: I did not find a SIGTERM or SIGINT handler in the parts of `retroarch.c` I read (the file was only
  partly readable). **Unverified** whether SIGTERM exits cleanly and writes SRAM. Test on the VM: start a game,
  make an in-game save, `kill -TERM`, check the `.srm` timestamp. If it does not save, use a sway `kill` on the
  window (a close request) or send the quit hotkey through the stream.
- Saves at exit: SRAM is written when content closes cleanly. `savestate_auto_save` (default false) writes a
  state at exit and `savestate_auto_load` (default true in the sample file) loads it on start; turning both on
  gives resume-where-you-left-off. `config_save_on_exit` (default true) rewrites `retroarch.cfg` on exit.
  Also `autosave_interval` (`DEFAULT_AUTOSAVE_INTERVAL` is 10 on some platforms and 0 on others; the Linux value
  is **unverified**) flushes SRAM periodically. Setting it to a few seconds protects against a kill.

## 2. Cores per system

Arch official repo (extra), from the Arch package search on 2026-10-28. Installed path is
`/usr/lib/libretro/<name>_libretro.so` (confirmed for `libretro-snes9x`: its file list is that one `.so` plus a
licence). The other cores follow the same layout; **unverified** per package. The `libretro-core-info` package
(1.22.2) provides the `.info` files RetroArch uses to list cores and their BIOS needs.

| System | Recommended | Alternatives | Arch package | Core file |
|---|---|---|---|---|
| NES / Famicom | Nestopia (cycle accurate, says 100% of released titles) | Mesen (RA listed); FCEUmm is RA's "most recommended" but is not in extra | `libretro-nestopia`, `libretro-mesen` | `nestopia_libretro.so`, `mesen_libretro.so` |
| SNES | Snes9x ("most recommended" by RA, fast, highly accurate) | bsnes (more accurate, needs a fast CPU), Mesen-S (RA listed) | `libretro-snes9x`, `libretro-bsnes`, `libretro-mesen-s` | `snes9x_libretro.so`, `bsnes_libretro.so`, `mesen-s_libretro.so` |
| Mega Drive / Genesis | Genesis Plus GX (RA "most recommended"; 32X not supported) | PicoDrive (RA listed; also 32X), BlastEm | `libretro-genesis-plus-gx`, `libretro-picodrive`, `libretro-blastem` | `genesis_plus_gx_libretro.so`, `picodrive_libretro.so`, `blastem_libretro.so` |
| Game Boy, GBC | Gambatte (accuracy focused; RA "most recommended") | SameBoy (accurate), mGBA (also does GB/GBC) | `libretro-gambatte`, `libretro-sameboy`, `libretro-mgba` | `gambatte_libretro.so`, `sameboy_libretro.so`, `mgba_libretro.so` |
| GBA | mGBA (RA "most recommended") | none in extra (VBA-M, Beetle GBA, VBA Next are not packaged) | `libretro-mgba` | `mgba_libretro.so` |

The exact core file names (underscores or dashes, for example `mesen-s_libretro.so`) are **unverified** for every
core except `snes9x`. The first VM step is `pacman -Ql libretro-<name>`.

Extensions, from the core docs: Nestopia `.nes .fds .unf .unif .nsf`; Snes9x `.smc .sfc .swc .fig .bs .st`;
Genesis Plus GX `.md .smd .gen .bin .sms .gg .sg ...` (also CD formats); Gambatte `.gb .gbc .dmg`; mGBA
`.gb .gbc .gba`. For the registry use `nes smc sfc swc fig md smd gen gb gbc gba` (and `bin` only if scans want it;
`.bin` is also used by CD images and many other things, so leave it out first).

Accuracy against performance: the games VM has no stated CPU limit for these 8 and 16 bit systems; every core above
runs full speed on any desktop CPU. bsnes is the only one that is CPU heavy. So pick for accuracy and RA support, not
speed. Defaults that follow from the lists: Nestopia, Snes9x, Genesis Plus GX, Gambatte, mGBA.

BIOS (all go in `system_directory`; none blocks a normal game):
- Nestopia: `disksys.rom` (MD5 `ca30b50f880eb660a320674ed365ef7a`) only for Famicom Disk System `.fds` games.
- Snes9x: none; `BS-X.bin`, `STBIOS.bin` only for BS-X and Sufami Turbo. bsnes: optional coprocessor firmware, `SGB1.sfc` / `SGB2.sfc` for Super Game Boy.
- Genesis Plus GX: none for cartridges. Sega CD needs `bios_CD_U.bin`, `bios_CD_E.bin`, `bios_CD_J.bin` (out of scope).
- Gambatte: `gb_bios.bin`, `gbc_bios.bin` optional, used only if "Use official bootloader" is on.
- mGBA: `gba_bios.bin` optional (and the GB ones); used only if "Use BIOS file if found" is on.

## 3. Controllers

- RetroArch maps real pads to a virtual RetroPad (d-pad, four face buttons, Select/Start, shoulders, triggers, two
  sticks). Known pads are matched by the autoconfig database (`.cfg` files per device, directory
  `joypad_autoconfig_dir`); on a match an on-screen message appears.
- The Arch `retroarch` file list has no autoconfig directory and I found no Arch package with "autoconfig" in
  its name. So on Arch the autoconfig profiles are not installed by pacman. Options: fetch the libretro
  `retroarch-joypad-autoconfig` repo once (not checked), use RetroArch's online updater (needs the menu), or
  have brisk write one small autoconfig file for the Sunshine pad.
- Sunshine's virtual pad on bawn is an Xbox Series profile via libvirtualhid and uinput (`docs/controller-notes.md`).
  RetroArch's udev joypad driver (`input_joypad_driver`, default empty = auto) reads uinput devices like any evdev
  pad. Whether the stock database has a matching profile for that vendor and product id is **unverified**. Even
  without one, RetroArch falls back to a generic mapping that is often right for Xbox-like pads, but the d-pad
  and triggers can be wrong, so test.
- An autoconfig file is a plain `key = "value"` file, named for the device, with `input_device`, `input_vendor_id`,
  `input_product_id`, and `input_a_btn`, `input_up_btn` and so on. **Unverified** key list; check an existing
  profile in the libretro repo before writing one.
- Remaps: Quick Menu, Controls, "Save Core Remap File" or "Save Game Remap File" (docs page). The files are
  `.rmp` text files with lines like `input_player1_btn_a = "1"`, where the number is the physical RetroPad index
  the core reads. Directory key: `input_remapping_directory` (confirmed in `configuration.c` and `retroarch.cfg`).
  Layout: `<remap dir>/<core name>/<core name>.rmp` for a core, and `<game name>.rmp` for one game (from forum
  and handheld-distro pages, not an official libretro page: **unverified** for the Arch default folder; I believe
  `~/.config/retroarch/config/remaps/`).
- Can a brisk controller editor write these? Yes in principle: remaps are tiny text files in a known folder, and
  `auto_remaps_enable` is on by default (`config.def.h`), so a core file is picked up without more config. But
  it is a different model from Dolphin's: Dolphin has a per-device INI (`GCPadNew.ini`, `WiimoteNew.ini`) with
  expressions for each emulated button; RetroArch has a fixed RetroPad and the remap only permutes it per core.
  So the existing `classic` and `gamecube` editor types do not fit. A new type like `retropad` with 12 buttons
  and per-core files would. Not needed for a first version: one RetroPad layout works for all five systems.
- Face button layout trap: RetroPad B is the bottom button, like SNES. On an Xbox pad that means Xbox A maps to
  RetroPad B. That is correct for Nintendo games and is what autoconfig does for Xbox pads.
- Hotkeys: set "Enable Hotkeys" to a pad button (for example Guide/Home or Select), then exit, save state, load
  state need that button held. Without it, only keyboard defaults (Esc exit, F2 save state, F4 load state, F1 menu)
  work, and a browser stream sends keyboard keys only if brisk forwards them.

## 4. Saves and states

- Defaults: `savefile_directory` and `savestate_directory` are empty in the sample `retroarch.cfg`. The directory
  docs page says that by default saves and states go "in the same folder as your ROMs" (content directory).
  `savefiles_in_content_dir` and `savestates_in_content_dir` are real bool keys. Both default values are
  **unverified** on Arch; the Qt/menu default may differ from the sample file.
- Files: SRAM is `<content name>.srm` (also `.rtc` and `.bsv` beside it, per the `retroarch.cfg` comment); states are
  `<content name>.state` for slot 0 and `.state1`, `.state2`... for others (the Gambatte page says `*.state#`);
  auto state is `.state.auto` (**unverified** name).
- Per-core folders: the config keys `sort_savefiles_enable`, `sort_savestates_enable` and the `..._by_content_enable`
  pair exist in `configuration.c` (as macros; exact strings **unverified**). With "sort by core" a folder named after
  the core is created under the directory. For brisk the cleanest setup is to set both directories in the append file
  to folders brisk owns, for example `<data dir>/retroarch/saves` and `<data dir>/retroarch/states`, with no sorting.
  The file name is the game file stem, so two systems with the same stem would collide. A per-system subfolder fixes
  that, if the sort option or a per-system append file provides it.
- Stage 6c list: for each game look in those two folders for `<file stem>.srm` and `<file stem>.state*`. Shape is
  simpler than Dolphin's (no memory cards, no title ids). Gambatte, mGBA and Snes9x all write `.srm`, but
  different cores can write different `.srm` layouts for the same game, so a save is tied to the core, not just the game.
  This is another reason to fix one core per system.

## 5. Achievements

- Login: Settings, Achievements: enable, enter user name and password in the menu (RA docs). The config keys exist in
  `configuration.c`: `cheevos_username`, `cheevos_password`, `cheevos_token` (all marked sensitive), plus `cheevos_enable`
  (default false) and `cheevos_hardcore_mode_enable` (default true in the sample file). After a login RetroArch keeps the
  token; the docs page says to clear it under Settings, User, Accounts, RetroAchievements if login suddenly fails because
  the token expired. Whether the password is removed from the file after login is **unverified**. Brisk should write only
  `cheevos_enable`, `cheevos_username` and, if it must, `cheevos_token` after one manual login, never the password.
- Hardcore: disables save states, slow motion, rewind-style helpers and cheats ("The hardcore mode prevents you from
  using emulation features like savestates, slow motion, and cheats"); needed for site events and its own leaderboard. This
  conflicts with a "save state" button in the stream UI: in hardcore, brisk should hide or disable it.
- Cores: the RA emulator support page lists, as hardcore-compliant libretro cores, FCEUmm, Mesen, QuickNES (NES);
  Snes9x, Mesen-S (SNES); Genesis Plus GX, PicoDrive (Mega Drive); Gambatte, Gearboy, mGBA, VBA-M (GB, GBC); mGBA, VBA-M,
  Beetle GBA, VBA Next (GBA); Mupen64Plus-Next, ParaLLEl N64 (N64); Beetle PSX HW, Beetle PSX, SwanStation (PSX). The libretro
  RA page says "Cores that are not listed as supported are likely to have issues with achievement logic". Note that
  Nestopia and bsnes (my picks for accuracy) are not on that list. For achievements on NES use `libretro-mesen`; for SNES
  Snes9x is fine. The Dolphin libretro core is marked unusable, as in `emulator-metadata-and-achievements.md`.
- Hashing (rcheevos / RA game identification page):

| System | Hash |
|---|---|
| NES | Custom, not MD5 of the whole file. If the file starts with `NES\x1a`, skip the first 16 bytes and hash the rest; otherwise hash the whole file. (The page says "custom"; the digest algorithm is MD5 in rcheevos, **unverified** on this page.) |
| SNES | MD5. If the size is 512 bytes more than a multiple of 8 KB, skip the first 512 bytes; else the whole file. |
| Mega Drive / Genesis | MD5, no header or byte-order rule on the page. |
| Game Boy, GBC, GBA | MD5, no further rule on the page (whole file implied). |
| N64 | MD5 of the big-endian `.z64` form; `.v64` (byte swapped) and `.n64` (little endian) are converted first. |
| PlayStation | Custom: parse `SYSTEM.CNF`, take the `BOOT=` executable name and path, put name plus contents in a buffer, hash it. |

- What the roadmap's `discid.rs` has to change: there is no `discid.rs` in this branch (`crates/brisk-hostd/src` has no such
  file; `grep` for SHA-1 finds only the roadmap text). The roadmap's SHA-1 is the fallback library id (first 10 hex chars of SHA-1
  of system plus lowercase file stem) and a Dolphin header reader. That id is for stable library ids and cannot be reused for
  RA, because RA needs MD5 over file contents, with these rules. What would be added: (a) the `md-5` crate (pure Rust; CLAUDE.md
  requires pure Rust product code); (b) a per-system function `ra_hash(system, bytes)`: NES strips the iNES header when the first four
  bytes are `4E 45 53 1A`, SNES strips 512 bytes when `len % 8192 == 512`, everything else is plain MD5 of the bytes (N64 and PSX are
  out of scope until the registry has those systems); (c) reading the whole file (cartridges are small, under 64 MB, so a
  single read is fine; zipped ROMs must be unpacked first, since RA hashes the ROM, not the zip; RetroArch itself loads zips, so the
  scan should either skip `.zip` or read the first entry); (d) the hash is looked up as in the existing research doc:
  `API_GetGameList.php?i=<system>&h=1` and match locally; (e) library id for retro files can stay as is, but caching
  the RA hash with the metadata saves recomputing.
- NES and the 16 byte header: other dumps may carry a different header; the page text only covers `NES\x1a`. Fine for a first version.

## 6. Proposed registry row

A Rust-like sketch, not code in the repo. The present `Emulator` struct has one `args` list and one `window_class`.
RetroArch needs the core to depend on the system, so `System` gains a `core` field and `args` stays emulator wide.

```rust
pub struct System {
    pub id: &'static str,
    pub folder: &'static str,
    /// Core file under the cores dir; empty for emulators with no cores (Dolphin).
    pub core: &'static str,
    pub extensions: &'static [&'static str], // per system, since one row spans five
}

Emulator {
    id: "retroarch",
    name: "RetroArch",
    cmd: "retroarch",                // also the process name (comm)
    // core comes first from the system; this tail follows it. The file goes last, as its own argv entry.
    args: &["-f", "--appendconfig", "<data dir>/retroarch/brisk.cfg"],  // path filled at run time
    window_class: "retroarch",       // or app_id "com.libretro.RetroArch"; settle on the VM
    controller: &[],                 // none yet; later "retropad"
    save_paths: SavePaths {          // for Stage 6c
        sram:  "<data dir>/retroarch/saves/<stem>.srm",
        state: "<data dir>/retroarch/states/<stem>.state*",
    },
    systems: &[
        System { id: "nes",       folder: "NES",        core: "nestopia_libretro.so",        extensions: &["nes", "fds", "unf", "unif"] },
        System { id: "snes",      folder: "SNES",       core: "snes9x_libretro.so",          extensions: &["smc", "sfc", "swc", "fig"] },
        System { id: "megadrive", folder: "Mega Drive", core: "genesis_plus_gx_libretro.so", extensions: &["md", "smd", "gen"] },
        System { id: "gb",        folder: "Game Boy",   core: "gambatte_libretro.so",        extensions: &["gb"] },
        System { id: "gbc",       folder: "Game Boy Color", core: "gambatte_libretro.so",    extensions: &["gbc"] },
        System { id: "gba",       folder: "Game Boy Advance", core: "mgba_libretro.so",      extensions: &["gba"] },
    ],
}
```

The append file `brisk.cfg` that hostd writes before each launch (it is a few lines):

```
savefile_directory = "<data dir>/retroarch/saves"
savestate_directory = "<data dir>/retroarch/states"
system_directory = "<bios dir>"
input_remapping_directory = "<data dir>/retroarch/remaps"
video_fullscreen = "true"
savestate_auto_save = "true"
savestate_auto_load = "true"
config_save_on_exit = "false"
# confirm-quit off: key name unverified, see section 1
```

Cores dir is `/usr/lib/libretro/` on Arch. Make it an env var or config entry (`docs/env.md` lists the others) and
check the core file exists before launch, as `launch` already checks the game file.

### Changes in `emu.rs`

Read from the current `emu.rs` (`launch`, `pids`, `caps`):
- `launch`: today it builds `args = e.args + file`. For RetroArch it must build `-L <cores dir>/<system core>` first, then
  `e.args`, then the file. That needs the record's `system` (already on `Record`) to look up the `System` row. Refuse to
  launch (return false, log why) when the core file is missing. Write `brisk.cfg` first.
- `pids` (running detection): it keeps processes whose `comm == e.cmd` and whose argv contains the game file. For
  `retroarch` this works as is, if the process `comm` is `retroarch` (the Arch binary is `/usr/bin/retroarch`; Linux truncates `comm` at 15
  characters, so `retroarch` fits). Argv holds the file path as an exact entry, so the equality test holds. Only a wrapper script that execs
  would hide it; none here.
- Focus: `stream::focus` takes the class from the source. For Xwayland that is `retroarch`. For native Wayland it must match on `app_id`
  `com.libretro.RetroArch`. Check how the focus code builds its sway criteria and whether a second string is cheap to add (`match_by`
  is already mentioned in the registry header comment as the planned extension).
- Quit: I found no quit or stop path in `emu.rs` (`grep` for `ConfirmStop` and `kill` in `crates/brisk-hostd/src` hits only the clipboard and stream
  code), so the Dolphin ConfirmStop setting is a concern of the Dolphin config the user sets, not hostd. If a quit path is added, RetroArch needs the
  polite route described in section 1.
- Extensions and scan: `extensions` moves from the emulator to the system so a scan can mark `.gba` as playable by `retroarch`
  without `.nes` also claiming `.gba`. The scan that today reports "unplayable: no emulator" for cartridge files then resolves the
  extension through the registry. Two systems that share an extension (`.bin`, `.gb` as GB or GBC under mGBA) need the folder name as a tie break.
- `caps`: `controls` is false while `controller` is empty. Right for a first version.

## 7. Open decisions for Toby

1. Install on the games VM: `pacman -S retroarch libretro-nestopia libretro-snes9x libretro-genesis-plus-gx libretro-gambatte libretro-mgba libretro-core-info`
   (all `extra`, names from the Arch search). Add `libretro-mesen` if NES achievements matter (RA lists Mesen, not Nestopia). Add `libretro-mesen-s` for SNES
   achievements only if Snes9x is not wanted (it is on RA's list already, so no). Root needed, so it goes through `sudoq`.
2. NES core: Nestopia (accurate, not RA listed) or Mesen (RA listed)? Suggest Mesen if achievements are a goal, else Nestopia.
3. One fixed core per system (the table above) or a per-system choice in the UI? Suggest fixed in the registry for now: a save is tied to the core,
   and a UI choice multiplies the test matrix. A later "core override" could live in the game's settings.
4. Window match: native Wayland or Xwayland? Settled by one test run on the VM.
5. Hardcore mode: on (default) hides the stream's save-state buttons and blocks cheats. Off gives states but RA marks the
   unlocks softcore. Needs a choice when Stage 5 starts.
6. Autoconfig for the Sunshine pad: fetch the libretro autoconfig repo once, or have brisk write one file.
7. BIOS folder: mostly not needed for these five systems. Only FDS needs `disksys.rom`; leave `system_directory` empty-but-defined.
8. Quit hotkey on the pad: pick the button for "Enable Hotkeys" and for exit, or only let brisk end the game.
9. Whether `.bin` and `.zip` ROMs are scanned at all.

## What I could not verify

- Whether a SIGTERM makes RetroArch exit and flush SRAM; the key name for confirm-quit; the Linux autosave default.
- Which window id (app_id or X11 class) an Arch build shows under sway.
- Core file names other than `snes9x_libretro.so`; the remaps default folder; autoconfig key names and whether a database profile fits the Sunshine pad.
- `sort_*` option strings; the exact `.state` and auto-state file names; the `.srm` default folder on Arch.
- That `libretro-fceumm`, `-vbam`, `-gearboy`, `-quicknes` are absent from official repos: the Arch search returned none, but I did not search the AUR.
- The NES digest algorithm (page says "custom", rcheevos is MD5 as far as I know).
