//! Non-Steam games from `data_dir/emulators.json`: `[{"name","emulator","file"}]`. They show up in
//! `/api/games` next to Steam's, under ids no Steam appid reaches (0xE0000000 and up, from a hash of
//! the name), and Play starts the emulator on the host like `steam()` starts Steam. Read tolerantly:
//! a missing or corrupt file is just no emulator games.
//! shortcut: Dolphin only, no running/quit tracking (a card never shows "running"); add both when a
//! second emulator or a stuck-window complaint arrives.

use crate::host::Runner;
use crate::steam::Game;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const ID_BASE: u32 = 0xE000_0000;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub emulator: String,
    pub file: PathBuf,
}

impl Entry {
    /// FNV-1a of the name folded into the emulator id range, so ids survive reordering the file.
    pub fn id(&self) -> u32 {
        let h = self.name.bytes().fold(0x811c_9dc5_u32, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        });
        ID_BASE | (h & 0x0FFF_FFFF)
    }
}

pub fn is_emu_id(id: u32) -> bool {
    id >= ID_BASE
}

pub fn load(data_dir: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(data_dir.join("emulators.json")) else {
        return vec![];
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("warn: emulators.json ignored: {e}");
        vec![]
    })
}

/// One card per entry; `installed` means the game file exists, so a missing ISO reads as "not installed".
pub fn games(entries: &[Entry]) -> Vec<Game> {
    entries
        .iter()
        .map(|e| Game {
            appid: e.id(),
            name: e.name.clone(),
            installed: e.file.is_file(),
            size_bytes: std::fs::metadata(&e.file).ok().map(|m| m.len()),
            last_played: None,
            playtime_min: 0,
            playtime2wk_min: 0,
            cover: String::new(),
            hero: String::new(),
            logo: String::new(),
            running: false,
            ach_unlocked: None,
            ach_total: None,
            updated: false,
            update: None,
            controller: None,
            coop: false,
        })
        .collect()
}

/// Starts the emulator on the game file, detached, in the graphical session. False for an unknown id,
/// a missing file, an emulator we don't know, or a failed start. Blocking.
pub fn launch(runner: &dyn Runner, entries: &[Entry], id: u32, env: &[(String, String)]) -> bool {
    let Some(e) = entries.iter().find(|e| e.id() == id) else {
        return false;
    };
    let (Some(file), "dolphin") = (
        e.file.to_str().filter(|_| e.file.is_file()),
        e.emulator.as_str(),
    ) else {
        return false;
    };
    // -b: no UI shell, -e: boot this file. The path is a separate argument, never parsed by a shell.
    runner.spawn("dolphin-emu", &["-b", "-e", file], env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Rec(Mutex<Vec<String>>);
    impl Runner for Rec {
        fn run(&self, _: &str, _: &[&str]) -> Option<String> {
            None
        }
        fn spawn(&self, p: &str, a: &[&str], _: &[(String, String)]) -> bool {
            self.0.lock().unwrap().push(format!("{p} {}", a.join(" ")));
            true
        }
    }

    fn entry(name: &str, emu: &str, file: &Path) -> Entry {
        Entry {
            name: name.into(),
            emulator: emu.into(),
            file: file.into(),
        }
    }

    #[test]
    fn ids_are_stable_and_clear_of_steam() {
        let e = entry("Mario Kart Wii", "dolphin", Path::new("/x"));
        assert_eq!(e.id(), e.clone().id());
        assert!(is_emu_id(e.id()) && !is_emu_id(730));
    }

    #[test]
    fn missing_file_reads_not_installed_and_will_not_launch() {
        let e = entry("Mario Kart Wii", "dolphin", Path::new("/nope/mkwii.iso"));
        assert!(!games(std::slice::from_ref(&e))[0].installed);
        let r = Rec(Mutex::new(vec![]));
        assert!(!launch(&r, std::slice::from_ref(&e), e.id(), &[]));
        assert!(r.0.lock().unwrap().is_empty());
    }

    #[test]
    fn launches_dolphin_on_an_existing_file_only() {
        let f = std::env::temp_dir().join(format!("brisk-emu-{}.iso", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        let ok = entry("MKW", "dolphin", &f);
        let other = entry("Other", "mystery", &f);
        let r = Rec(Mutex::new(vec![]));
        assert!(games(std::slice::from_ref(&ok))[0].installed);
        assert!(launch(&r, &[ok.clone(), other.clone()], ok.id(), &[]));
        assert!(!launch(&r, &[ok.clone(), other.clone()], other.id(), &[]));
        assert!(!launch(&r, std::slice::from_ref(&ok), 730, &[]));
        assert_eq!(
            *r.0.lock().unwrap(),
            vec![format!("dolphin-emu -b -e {}", f.display())]
        );
        std::fs::remove_file(f).ok();
    }

    #[test]
    fn corrupt_or_missing_file_is_empty() {
        let d = std::env::temp_dir().join(format!("brisk-emu-d-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(load(&d).is_empty());
        std::fs::write(d.join("emulators.json"), "{not json").unwrap();
        assert!(load(&d).is_empty());
        std::fs::write(
            d.join("emulators.json"),
            r#"[{"name":"A","emulator":"dolphin","file":"/a.iso"}]"#,
        )
        .unwrap();
        assert_eq!(load(&d).len(), 1);
        std::fs::remove_dir_all(d).ok();
    }
}
