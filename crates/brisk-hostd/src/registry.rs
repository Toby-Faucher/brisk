//! The emulators brisk can run: a static table, one row each. A second emulator is a new row
//! (and a header reader when its files carry an id). Where Dolphin keeps its config is
//! `Config::dolphin_dir`, which the controller editor takes from there.
//! shortcut: one window class per emulator and no pid match for the window; add `match_by` to a
//! row if two emulators ever share a class.

/// A system an emulator plays; `folder` is the games-folder subfolder (used by library ingest later).
pub struct System {
    pub id: &'static str,
    pub folder: &'static str,
}

pub struct Emulator {
    pub id: &'static str,
    pub name: &'static str,
    /// The command, which is also its process name.
    pub cmd: &'static str,
    /// Arguments before the game file, which follows as its own argv entry.
    pub args: &'static [&'static str],
    /// The X11 class of its window (sway shows Xwayland windows by class, not app_id).
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
            },
            System {
                id: "wii",
                folder: "Wii",
            },
        ],
        extensions: &[
            "iso", "rvz", "wbfs", "gcm", "ciso", "gcz", "wia", "wad", "tgc",
        ],
    },
    Emulator {
        id: "azahar",
        name: "Azahar",
        cmd: "azahar",
        // -f: fullscreen (the Qt frontend's own flag); the game path is the last argument.
        args: &["-f"],
        // UNVERIFIED: Qt's X11 class is the application name; check with swaymsg on the VM.
        window_class: "azahar",
        // No controller editor: the input mapping format is not verified (docs/research/azahar-notes.md).
        controller: &[],
        systems: &[System {
            id: "3ds",
            folder: "3DS",
        }],
        extensions: &["3ds", "cci", "cxi", "cia", "3dsx", "app"],
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
        let a = get("azahar").unwrap();
        assert_eq!((a.cmd, a.args), ("azahar", &["-f"][..]));
        assert!(a.controller.is_empty() && a.systems[0].id == "3ds");
        assert!(get("mystery").is_none());
        let ids: Vec<_> = EMULATORS.iter().map(|e| e.id).collect();
        assert!(ids.iter().all(|i| {
            !i.is_empty()
                && i.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        }));
    }
}
