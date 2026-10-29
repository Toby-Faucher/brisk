//! The emulators brisk can run: a static table, one row each. A second emulator is a new row
//! (and a header reader when its files carry an id). Where Dolphin keeps its config is
//! `Config::dolphin_dir`, which the controller editor takes from there.
//! shortcut: one window class per emulator and no pid match for the window; add `match_by` to a
//! row if two emulators ever share a class.

/// A system an emulator plays; `folder` is the games-folder subfolder (used by library ingest later).
pub struct System {
    pub id: &'static str,
    pub folder: &'static str,
    /// The libretro core file for this system (RetroArch: `-L <cores dir>/<core>`); empty = none.
    pub core: &'static str,
}

pub struct Emulator {
    pub id: &'static str,
    pub name: &'static str,
    /// The command, which is also its process name.
    pub cmd: &'static str,
    /// Arguments before the game file, which follows as its own argv entry. A system with a `core`
    /// gets `-L <core path>` ahead of these (see `retroarch::argv`).
    pub args: &'static [&'static str],
    /// The X11 class of its window (sway shows Xwayland windows by class, not app_id). A native
    /// Wayland window matches too when its app_id equals this or ends in `.<this>`, any case.
    pub window_class: &'static str,
    /// Controller editor types it supports; empty = no editor.
    pub controller: &'static [&'static str],
    pub systems: &'static [System],
    pub extensions: &'static [&'static str],
}

pub static EMULATORS: &[Emulator] = &[
    Emulator {
        id: "dolphin",
        name: "Dolphin",
        cmd: "dolphin-emu",
        // -b: no UI shell, -e: boot this file.
        args: &["-b", "-e"],
        window_class: "dolphin-emu",
        controller: &["classic", "gamecube"],
        systems: &[
            System {
                id: "gamecube",
                folder: "GameCube",
                core: "",
            },
            System {
                id: "wii",
                folder: "Wii",
                core: "",
            },
        ],
        extensions: &[
            "iso", "rvz", "wbfs", "gcm", "ciso", "gcz", "wia", "wad", "tgc",
        ],
    },
    // One fixed core per system (a save is tied to its core). Core file names are unverified
    // except snes9x: docs/plans/2026-10-28-retroarch-impl.md. The window id (X11 class or Wayland
    // app_id `com.libretro.RetroArch`) is also unverified; the class match covers both.
    Emulator {
        id: "retroarch",
        name: "RetroArch",
        cmd: "retroarch",
        // -f: fullscreen. The game file follows.
        args: &["-f"],
        window_class: "retroarch",
        controller: &[],
        systems: &[
            System {
                id: "nes",
                folder: "NES",
                core: "mesen_libretro.so",
            },
            System {
                id: "snes",
                folder: "SNES",
                core: "snes9x_libretro.so",
            },
            System {
                id: "megadrive",
                folder: "Mega Drive",
                core: "genesis_plus_gx_libretro.so",
            },
            System {
                id: "gb",
                folder: "Game Boy",
                core: "gambatte_libretro.so",
            },
            System {
                id: "gbc",
                folder: "Game Boy Color",
                core: "gambatte_libretro.so",
            },
            System {
                id: "gba",
                folder: "Game Boy Advance",
                core: "mgba_libretro.so",
            },
        ],
        // No .bin or .zip: too ambiguous for a scan.
        extensions: &[
            "nes", "smc", "sfc", "swc", "fig", "md", "smd", "gen", "gb", "gbc", "gba",
        ],
    },
];

pub fn get(id: &str) -> Option<&'static Emulator> {
    EMULATORS.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dolphin_row_matches_what_launch_used_before() {
        let d = get("dolphin").unwrap();
        assert_eq!(
            (d.cmd, d.args, d.window_class),
            ("dolphin-emu", &["-b", "-e"][..], "dolphin-emu")
        );
        assert!(get("mystery").is_none());
        // every system id is unique across rows (ingest picks the emulator by system id)
        let mut sys: Vec<_> = EMULATORS
            .iter()
            .flat_map(|e| e.systems)
            .map(|s| s.id)
            .collect();
        let n = sys.len();
        sys.sort_unstable();
        sys.dedup();
        assert_eq!(sys.len(), n);
        let ids: Vec<_> = EMULATORS.iter().map(|e| e.id).collect();
        assert!(ids.iter().all(|i| {
            !i.is_empty()
                && i.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        }));
    }
}
