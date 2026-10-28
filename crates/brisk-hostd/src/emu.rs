//! Non-Steam games from `data_dir/emulators.json`: `[{"name","emulator","file"}]`. They show up in
//! `/api/games` next to Steam's, under ids no Steam appid reaches (0xE0000000 and up, from a hash of
//! the name), and Play starts the emulator on the host like `steam()` starts Steam. Read tolerantly:
//! a missing or corrupt file is just no emulator games.
//! A game is running when a process of the emulator's command has the game file as an argument (a
//! scan of the proc dir, so it survives a hostd restart). Play on a running game only focuses it.
//! shortcut: Dolphin only (one table row in `spec`), one window class per emulator; add a row per
//! emulator, and match the window by pid too if two emulators ever share a class.

use crate::host::Runner;
use crate::running::{Proc, procs};
use crate::steam::Game;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

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

/// How one emulator runs: its command (also its process name), the arguments before the game
/// file, and its window's X11 class (sway shows Xwayland windows by class, not app_id).
pub struct Spec {
    pub cmd: &'static str,
    args: &'static [&'static str],
    pub class: &'static str,
    /// Has a controller editor (`controls.rs`).
    pub controls: bool,
}

pub fn spec(emulator: &str) -> Option<Spec> {
    // -b: no UI shell, -e: boot this file.
    (emulator == "dolphin").then_some(Spec {
        cmd: "dolphin-emu",
        args: &["-b", "-e"],
        class: "dolphin-emu",
        controls: true,
    })
}

/// Pids of the emulator processes that were started on this entry's file.
pub(crate) fn pids(ps: &[Proc], e: &Entry) -> HashSet<u32> {
    let (Some(s), Some(file)) = (spec(&e.emulator), e.file.to_str()) else {
        return HashSet::new();
    };
    ps.iter()
        .filter(|p| p.comm == s.cmd && p.argv.iter().any(|a| a == file))
        .map(|p| p.pid)
        .collect()
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
pub fn games(entries: &[Entry], proc: &Path) -> Vec<Game> {
    let ps = procs(proc);
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
            running: !pids(&ps, e).is_empty(),
            ach_unlocked: None,
            ach_total: None,
            updated: false,
            update: None,
            controller: None,
            coop: false,
        })
        .collect()
}

/// Starts the emulator on the game file, detached, in the graphical session, unless it already
/// runs that file: then nothing is spawned. True when the game is running afterwards. False for an
/// unknown id, a missing file, an emulator we don't know, or a failed start. Blocking.
pub fn launch(
    runner: &dyn Runner,
    proc: &Path,
    entries: &[Entry],
    id: u32,
    env: &[(String, String)],
) -> bool {
    let Some(e) = entries.iter().find(|e| e.id() == id) else {
        return false;
    };
    let (Some(file), Some(s)) = (
        e.file.to_str().filter(|_| e.file.is_file()),
        spec(&e.emulator),
    ) else {
        return false;
    };
    if !pids(&procs(proc), e).is_empty() {
        eprintln!("info: {} is already running; focusing it", e.name);
        return true;
    }
    // The path is a separate argument, never parsed by a shell.
    let args: Vec<&str> = s.args.iter().copied().chain([file]).collect();
    runner.spawn(s.cmd, &args, env)
}

/// Waits up to `wait` for the entry's emulator window, then focuses and fullscreens it. A miss is
/// only logged; the game stays as it is.
pub async fn focus(
    runner: Arc<dyn Runner>,
    entries: &[Entry],
    id: u32,
    env: Vec<(String, String)>,
    wait: Duration,
) {
    let Some(e) = entries.iter().find(|e| e.id() == id) else {
        return;
    };
    if let Some(s) = spec(&e.emulator) {
        crate::running::focus_class(runner, env, s.class, e.name.clone(), wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records spawns and swaymsg calls. `get_tree` shows a Dolphin window (con 40) from the
    /// `after`-th call on.
    struct Rec {
        log: Mutex<Vec<String>>,
        trees: Mutex<u32>,
        after: u32,
    }
    impl Rec {
        fn new(after: u32) -> Arc<Rec> {
            Arc::new(Rec {
                log: Mutex::default(),
                trees: Mutex::default(),
                after,
            })
        }
        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }
    impl Runner for Rec {
        fn run(&self, p: &str, a: &[&str]) -> Option<String> {
            if a.ends_with(&["-t", "get_tree"]) {
                let mut n = self.trees.lock().unwrap();
                *n += 1;
                let win = r#"{"id":40,"type":"con","window_properties":{"class":"dolphin-emu"}}"#;
                let other = r#"{"id":7,"type":"con","window_properties":{"class":"steam"}}"#;
                let nodes = if *n >= self.after { win } else { other };
                return Some(format!(r#"{{"id":1,"type":"root","nodes":[{nodes}]}}"#));
            }
            self.log
                .lock()
                .unwrap()
                .push(format!("{p} {}", a.join(" ")));
            Some(String::new())
        }
        fn spawn(&self, p: &str, a: &[&str], _: &[(String, String)]) -> bool {
            self.log
                .lock()
                .unwrap()
                .push(format!("spawn {p} {}", a.join(" ")));
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

    /// A fake /proc with one process per `(pid, comm, argv)`.
    fn fake_proc(procs: &[(u32, &str, &[&str])]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (pid, comm, argv) in procs {
            let p = d.path().join(pid.to_string());
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("cmdline"), argv.join("\0") + "\0").unwrap();
            let stat = format!("{pid} ({comm}) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 5 0 0");
            std::fs::write(p.join("stat"), stat).unwrap();
        }
        d
    }

    fn iso(tag: &str) -> PathBuf {
        let f = std::env::temp_dir().join(format!("brisk-emu-{tag}-{}.iso", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        f
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
        let none = fake_proc(&[]);
        assert!(!games(std::slice::from_ref(&e), none.path())[0].installed);
        let r = Rec::new(0);
        assert!(!launch(
            &*r,
            none.path(),
            std::slice::from_ref(&e),
            e.id(),
            &[]
        ));
        assert!(r.log().is_empty());
    }

    #[test]
    fn launches_dolphin_on_an_existing_file_only() {
        let f = iso("one");
        let ok = entry("MKW", "dolphin", &f);
        let other = entry("Other", "mystery", &f);
        let none = fake_proc(&[]);
        let r = Rec::new(0);
        let both = [ok.clone(), other.clone()];
        assert!(games(std::slice::from_ref(&ok), none.path())[0].installed);
        assert!(launch(&*r, none.path(), &both, ok.id(), &[]));
        assert!(!launch(&*r, none.path(), &both, other.id(), &[]));
        assert!(!launch(
            &*r,
            none.path(),
            std::slice::from_ref(&ok),
            730,
            &[]
        ));
        assert_eq!(
            r.log(),
            vec![format!("spawn dolphin-emu -b -e {}", f.display())]
        );
        std::fs::remove_file(f).ok();
    }

    #[test]
    fn running_means_a_dolphin_process_on_that_file_and_a_second_play_spawns_nothing() {
        let (a, b) = (iso("a"), iso("b"));
        let (ea, eb) = (entry("A", "dolphin", &a), entry("B", "dolphin", &b));
        let (sa, sb) = (a.to_str().unwrap(), b.to_str().unwrap());
        let p = fake_proc(&[
            (10, "dolphin-emu", &["dolphin-emu", "-b", "-e", sa]),
            // Not Dolphin, though it has B's path: must not count.
            (11, "vim", &["vim", sb]),
        ]);
        let es = [ea.clone(), eb.clone()];
        let g = games(&es, p.path());
        assert_eq!((g[0].running, g[1].running), (true, false));
        let r = Rec::new(0);
        // A is running: Play succeeds without a spawn. B is not: it spawns.
        assert!(launch(&*r, p.path(), &es, ea.id(), &[]));
        assert!(r.log().is_empty());
        assert!(launch(&*r, p.path(), &es, eb.id(), &[]));
        assert_eq!(r.log(), vec![format!("spawn dolphin-emu -b -e {sb}")]);
        std::fs::remove_file(a).ok();
        std::fs::remove_file(b).ok();
    }

    #[tokio::test(start_paused = true)]
    async fn focus_polls_until_the_window_appears_then_focuses_and_fullscreens() {
        let e = entry("MKW", "dolphin", Path::new("/x.iso"));
        let r = Rec::new(4);
        let env = vec![("SWAYSOCK".to_string(), "/s".to_string())];
        focus(
            r.clone(),
            std::slice::from_ref(&e),
            e.id(),
            env,
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(*r.trees.lock().unwrap(), 4);
        assert_eq!(
            r.log(),
            vec!["swaymsg -s /s [con_id=40] focus, fullscreen enable"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn focus_gives_up_cleanly_when_no_window_ever_appears() {
        let e = entry("MKW", "dolphin", Path::new("/x.iso"));
        let r = Rec::new(u32::MAX);
        focus(
            r.clone(),
            std::slice::from_ref(&e),
            e.id(),
            vec![],
            Duration::from_secs(60),
        )
        .await;
        assert!(r.log().is_empty(), "nothing is focused: {:?}", r.log());
        assert!(*r.trees.lock().unwrap() >= 60);
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
