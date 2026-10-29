# Azahar notes

Azahar is the maintained 3DS emulator descended from Citra. Read on 2026-10-29 from its source on GitHub
(`azahar-emu/azahar`, master; latest release 2126.2, 2026-10-08), the AUR, and 3dbrew.org. Nothing was installed or run:
the games VM was only asked which packages it has (read-only). Lines marked UNVERIFIED were not confirmed from a primary source.

## Package and install

- Arch: `pacman -Ss azahar` and the archlinux.org package search find nothing: it is **not in extra**.
- AUR (queried through the AUR RPC): `azahar` 1:2126.2-1 (builds from the release source tarball), `azahar-appimage`,
  `azahar-appimage-wayland`, `azahar-git`, `libretro-azahar` (a RetroArch core). The `azahar` PKGBUILD depends on glibc,
  qt6-base, crypto++, fmt, glslang, libusb, openal, openssl, sdl2, soundtouch, qt6-multimedia, zydis, enet, boost-libs and
  others, and makedepends on cmake, ninja, ffmpeg4.4, qt6-tools. It is a **long C++ build**, which the VM rules forbid on
  the VM now; a build elsewhere then `pacman -U` the package is the clean way (needs Toby's say-so).
- The PKGBUILD rewrites the desktop file to `Exec=env QT_QPA_PLATFORM=xcb azahar %f`: the maintainer forces X11 (Xwayland).
- Upstream recommends the **Flatpak** (Flathub `org.azahar_emu.Azahar`) and offers `azahar.AppImage` / `azahar-wayland.AppImage`
  on the releases page. The Flatpak has native Wayland off by default (README). The VM has no `flatpak`.
- Games VM today (read-only check): no `azahar`, no `flatpak`; `sdl2-compat`, `qt6-base` and `vulkan-radeon` are installed.
  Binary name from a source build or the AUR package: `/usr/bin/azahar`. An AppImage would not have the process name `azahar`
  (brisk's running check matches the process name), so an AppImage needs a wrapper or a registry change.
- License GPL-2.0-or-later (AUR) / GPL-3.0 (README says GPL-3.0; the source headers say GPLv2 or later). Unresolved, irrelevant to brisk.

## Launching and quitting (source: `src/citra_qt/citra_qt.cpp`, `src/citra_meta/main.cpp`)

- `azahar [options] <game path>`. A lone argument is the game; otherwise the **last** argument is the game.
  `-f` / `--fullscreen`, `-w` / `--windowed`, `-g/--gdbport`, `-i/--install <cia>`, `-p/--movie-play`, `-r/--movie-record`,
  `-d/--dump-video`, `-v/--version`, `-h/--help`. brisk row: `azahar -f <file>`.
- The Qt window starts fullscreen with `-f`. `Esc` exits fullscreen, `F11` toggles, `Ctrl+Q` is "Exit Azahar", `F5` stops
  emulation, `F9` swaps screens, `F10` toggles layout (default hotkeys in `config.cpp`).
- **Quit:** closing the window runs `closeEvent`; if `confirm_before_closing` is true (the default, `UISettings` key `confirmClose`)
  it shows a "Would you like to exit now?" box, defaulting to No. Nobody can answer that on a stream, so a `ConfirmStop`
  equivalent is needed: set `confirmClose=false` under `[UI]` in `qt-config.ini` (key name from `Settings::Keys::confirmClose`;
  the INI group name `UI` is UNVERIFIED). I found **no SIGTERM/SIGINT handler** in `citra_qt.cpp`, so SIGTERM probably kills
  the process without the orderly `ShutdownGame()` + `config->Save()` in `closeEvent` (UNVERIFIED by running). In-game saves
  are written by the emulated game through the SD folder as it saves, so a kill loses only unsaved game progress and
  the settings changes. brisk's Quit closes the window through sway (same as Dolphin), which takes the clean path once the
  confirm box is off.
- Window class: UNVERIFIED. Qt on X11 uses the application name, likely `azahar`; Wayland app_id probably `org.azahar_emu.Azahar`
  or `azahar`. Check with `swaymsg -t get_tree` on the VM before relying on it (the row uses `azahar`).

## Config and data layout (source: `common_paths.h`, `file_util.cpp`, `config.cpp`)

- Linux user dir: `$XDG_DATA_HOME/azahar-emu` (default `~/.local/share/azahar-emu`), config dir `$XDG_CONFIG_HOME/azahar-emu`
  (default `~/.config/azahar-emu`), cache `$XDG_CACHE_HOME/azahar-emu`. Legacy Citra/Lime3DS dirs are migrated. If the
  user dir `user/` portable folder exists next to the binary it wins (`USERDATA_DIR "user"`; details not read).
- Subdirs of the user dir: `config/`, `cache/`, `sdmc/`, `nand/`, `sysdata/`, `log/`, `cheats/`, `shaders/`, `dump/`, `load/`, `states/`.
  (The code has a portable and an XDG branch for the config dir; which one a given build uses is UNVERIFIED.)
- Global config `qt-config.ini` in the config dir; **per-game config** `config/custom/<name>.ini` (`QtConfig::Initialize`,
  `PerGameConfig`; `<name>` is the program id in hex, UNVERIFIED). It is a Qt `QSettings` INI.
- Fullscreen: `[UI] fullscreen` (`UISettings::fullscreen`), but `-f` overrides it for a run.
- Screen layout key `layout_option`: Default, SingleScreen, LargeScreen, SideScreen, SeparateWindows, HybridScreen, CustomLayout;
  also `swap_screen`, `upright_screen`. `SeparateWindows` opens a second top-level window.

### Input (source: `config.cpp`, `settings.h`, `sdl_impl.cpp`)

- Input lives in `[Controls]`; profiles in an INI array `profiles` (each: `name`, `button_a`..`button_power`, `circle_pad`,
  `c_stick`, `motion_device`, `touch_device`, `use_touchpad`, `controller_touch_device`, `udp_*`). Button keys:
  `button_a b x y up down left right l r start select debug gpio14 zl zr home power`. Analogs: `circle_pad`, `c_stick`.
- A binding value is a `ParamPackage` string such as `engine:sdl,guid:<sdl guid>,port:0,button:<n>`; axes use
  `axis:<n>,direction:+,threshold:0.5`; hats use `hat:<n>,direction:up`. Keyboard defaults otherwise.
  Gamepads come from SDL2 (game-controller API; indexes are `SDL_CONTROLLER_BUTTON_*`). The guid is the SDL joystick GUID string
  of the pad; Sunshine's virtual Xbox Series pad has one that must be read off the VM while a stream is connected
  (not done: no pad was probed so as not to disturb the running game).
- **Not implemented in brisk.** The exact escaping of those strings inside `QSettings` INI (commas and colons), the profile array
  layout and the `maptype` field were not verified against a real file, and writing a wrong mapping silently breaks input. Rule
  from the task: stub until verified. `controls.rs` is untouched; the registry row has `controller: &[]`, so the capability
  `controls` is false and no editor is offered for Azahar. To finish it: install once, map the pad in the GUI, copy the resulting
  `[Controls]` into `deploy/azahar/`, and port `controls.rs`'s read/write to that format.

## Save data and states (source: `savestate.cpp`, `archive_source_sd_savedata.cpp`, `archive_extsavedata.cpp`)

- Save states: `<user>/states/<ProgramID 16 hex UPPER>.<NN:02d>.cst`, `SaveStateSlotCount = 11` (slots 0..10; slot 0 is likely the
  quick-save, UNVERIFIED).
- SD save data: `<user>/sdmc/Nintendo 3DS/<32 zeros>/<32 zeros>/title/<hi:08x>/<lo:08x>/data/00000001/` (the id0/id1 folders are
  fixed all-zero constants in Azahar, `SYSTEM_ID`/`SDCARD_ID`). `hi`/`lo` are the halves of the program id, lowercase.
- Extra data: `.../extdata/<hi:08x>/<lo:08x>/` keyed by an extdata id that need not equal the program id; not listed by brisk.
- brisk lists states and the SD save folder read-only (`saves::list_azahar`).

## File formats and identification (source: `loader.cpp`, `ncch_container.h`; layout: 3dbrew.org/wiki/NCSD, /NCCH, /CIA, /Title_metadata)

- Extensions Azahar recognises (`GuessFromExtension`): `.3ds .cci .zcci .bcci` (NCSD), `.cxi .app .zcxi .bcxi` (NCCH), `.3dsx .z3dsx`,
  `.cia .zcia .bcia`, plus `.elf .axf`. Z/B variants are Azahar's compressed and bundle forms; brisk does not read them.
- Type is decided by content, not extension (`loader.cpp`: reads the magic at 0x100: `NCSD` or `NCCH`).
- NCSD (.3ds/.cci): magic `NCSD` at 0x100; partition table at 0x120 (8 x offset,length u32 LE in 0x200-byte units); partition 0 is
  the game's NCCH. The partition flags byte 6 (0x18E) is the media-unit exponent on paper; Azahar uses a fixed 0x200, as does brisk.
- NCCH header (plain, never encrypted): magic `NCCH` at 0x100, program id (title id) u64 LE at 0x118, product code 16 bytes at
  0x150 (`CTR-P-AREE`: the last four letters are the game code, and the final letter the region), flags at 0x188, `NoCrypto` is
  bit 0x04 of the byte at 0x18F. The game **name** is in the SMDH in ExeFS (encrypted for retail dumps), so it is not in the
  header: brisk names the library entry after the file.
- CIA: header size 0x2020 at 0, cert chain size at 8, ticket size at 0xC, TMD size at 0x10, sections padded to 0x40: header,
  cert chain, ticket, TMD. TMD: signature type u32 (BE) then signature and padding (RSA-2048 SHA256 = 0x100 + 0x3C), then the
  TMD header where the title id is at +0x4C. UNVERIFIED: that the TMD fields are big-endian (3dbrew layout read, not tested on a file).
- 3DSX: homebrew, magic `3DSX`, no title id; brisk keys it by SHA-1 like a cartridge.
- brisk id: `emu:azahar:<program id, 16 hex UPPER>`. Program ids differ per region, so a game in two regions is two entries.
  The 4-letter game code (the GameTDB id) is in the product code; brisk does not store it yet (no field on `Identity`).

## What the user must supply

Azahar plays decrypted ROMs unmodified. Encrypted retail dumps need Nintendo key material in the user dir's `sysdata/`
(`keys.txt` per `KEYS_FILE`, also `boot9.bin` and other console-unique files for some features; exact required set UNVERIFIED).
Brisk does not obtain, include, or check any of it. The user supplies game files they already have and any keys/system files
Azahar asks for.

## Achievements

RetroAchievements: rcheevos lists a console name `RC_CONSOLE_NINTENDO_3DS` (id 62) but its hash code (`rc_hash.c`) has no 3DS
case, and Azahar's `externals/` has no rcheevos. Treat 3DS as **unsupported**. brisk's achievements route answers
`{"state":"unsupported","reason":"RetroAchievements has no 3DS support in Azahar"}` (when a RA key is configured; otherwise "off").
The card's `achievements` cap stays false (it is Wii/GameCube only).

## Art and metadata

- SteamGridDB works by game name (brisk's existing path); the file-name-based name is used. No 3DS-specific stripping of
  suffixes like "(USA)" beyond what exists.
- GameTDB: `art.gametdb.com` hosts 3DS art (`https://art.gametdb.com/3ds/cart/US/AREE.png`, from `gametdb-notes.md`), with 4-letter
  ids. The site itself answered 403 to the fetch tool, so the 3DS database file name and URL are UNVERIFIED. brisk's GameTDB code
  is Wii/GameCube-only (6-char ids), so 3DS gets no GameTDB data yet.

## Dual screen, touch and Sunshine

- The 3DS has two screens: top 400x240 (800x240 in 3D mode), bottom 320x240 touch. Default layout stacks both in one window, so one
  Sunshine capture covers both. `SeparateWindows` would create two top-level windows, which a single-window sway kiosk capture
  would show badly; keep the default or a single-window layout (`layout_option`).
- Touch: the bottom screen needs a pointer. Azahar's `touch_device` default is `engine:emu_window` (the mouse on the window), which
  Moonlight/Sunshine delivers as mouse or touch events; a gamepad alone cannot tap the lower screen. Alternatives in the config: touch from
  buttons (`use_touch_from_button`) and the controller touchpad (`use_touchpad`, `controller_touch_device`). The Sunshine virtual pad
  has no touchpad (UNVERIFIED).
- Games that need the touch screen (most menus, many titles) are awkward on a pad-only client. Gyro (`motion_device`) and the
  microphone and camera are emulated; the outer-camera AR titles need a source.
- Resolution scales (internal resolution setting); on a 4K stream 3DS is cheap to render but the two-screen layout leaves black bars.
  Hybrid/large layouts may suit a 16:9 stream. A decision for Toby.
- The emulator runs a game thread plus a Qt window; the Vulkan renderer is the default on AMD (UNVERIFIED).

## What was built in brisk (branch `exp/azahar`)

Registry row `azahar` (3ds, extensions above, `azahar -f`, no controller editor); `discid` reads NCSD, NCCH, CIA and 3DSX headers
(tests use synthetic bytes); scan/upload accept the extensions and file the game under `3ds`; records get the id above; saves list
Azahar's states and SD save folder (`BRISK_AZAHAR_USER_DIR`); running/playtime work through the existing process-name plus game-path
argument match (the game path is the last argument, as the process watcher expects); art is by name; achievements are unsupported.

## UI that would still be needed (no UI was changed)

- Emulators screen: an Azahar entry with Games folders/Launch options/Art rows should follow the registry; Controllers should
  say "not available yet". Check `Emulators.svelte`/`registry.ts` (they are Dolphin-specific today, UNVERIFIED for Azahar).
- Saves rows: the web labels a `nand-save` row "Wii save" (`library.ts` `saveRow`); a 3DS save needs its own label.
- A "3DS" system badge/art aspect: 3DS box art is not 1:1.
- Touch: a stream touch/mouse mode toggle, and a note on the game detail that this title needs the touch screen.
- Achievements panel: show the plain `unsupported` reason (already shown for other unsupported cases).

## Deploy and VM install (not done)

1. Merge, rebuild hostd, redeploy to the games VM (copy `target/release` + `web/dist`, per the VM memory note).
2. Install Azahar on the VM: the AUR `azahar` needs a build (or an AppImage/Flatpak): pick one; then run it once to create the
   config, set `confirmClose=false`, layout, and map the pad in the GUI.
3. Put the user's 3DS files in `~/roms/3ds` (or the games dir `3ds` folder) and rescan; supply keys if the dumps are encrypted.
4. Set `BRISK_AZAHAR_USER_DIR` only if the user dir is not the XDG default.
