//! Emulator games: the library file `data_dir/library.json` and how a game runs. The emulator
//! itself comes from the registry (`registry.rs`); `source.rs` puts this behind `GameSource`.
//!
//! `library.json` is `{"v":1,"games":[Record]}`. When it is absent the old hand-written
//! `emulators.json` (`[{"name","emulator","file"}]`) is read instead and converted in memory
//! (`migrate::library_from_emulators`); startup writes the converted file and never deletes the
//! old one. A missing or corrupt file is just no emulator games.
//! Last-played times (unix seconds, by game id) live in `data_dir/emu-played.json`, written at
//! launch. Playtime is not tracked here (roadmap Stage 6a).
//! A game is running when a process of the emulator's command has the game file as an argument (a
//! scan of the proc dir, so it survives a hostd restart). Play on a running game only focuses it.

use crate::game::{Caps, Game, GameId, Source};
use crate::host::Runner;
use crate::launchstore::{command_for, plain, read_file};
use crate::registry;
use crate::running::{Proc, procs};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const LIBRARY_FILE: &str = "library.json";
pub const OLD_FILE: &str = "emulators.json";

/// One game in the library. `id` is explicit and stable: a later stage may replace it (a disc
/// header id) and keep the old one in `aliases`, so notes saved under it still resolve.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: GameId,
    pub emulator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub file: PathBuf,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<u64>,
    /// "manual" (hand-written), later "scan" and "upload".
    #[serde(default = "manual")]
    pub source: String,
    /// The old 0xE0000000+ id, from the name hash. Absent: it is computed from the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<GameId>,
}

fn manual() -> String {
    "manual".into()
}

/// FNV-1a of the name folded into the old emulator id range: the first emulator version's id.
pub fn legacy_hash(name: &str) -> u32 {
    let h = name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    crate::game::LEGACY_EMU_BASE | (h & 0x0FFF_FFFF)
}

impl Record {
    /// The old numeric id this game answered to.
    pub fn legacy(&self) -> u32 {
        self.legacy_id.unwrap_or_else(|| legacy_hash(&self.name))
    }

    /// Matches the current id or one it replaced.
    pub fn is(&self, id: &GameId) -> bool {
        self.id == *id || self.aliases.contains(id)
    }
}

/// The 6-char disc id of a record: the first alphanumeric 6-char tail among its `id` and `aliases`.
/// A scan keeps an older slug id and stores the disc id as an alias, so ids alone are not enough.
/// Saves use it; art and achievements can too.
pub fn disc_id_of(r: &Record) -> Option<String> {
    std::iter::once(&r.id)
        .chain(&r.aliases)
        .find_map(crate::saves::disc_id)
}

/// The record `id` names, if the library has it.
pub fn find<'a>(recs: &'a [Record], id: &GameId) -> Option<&'a Record> {
    recs.iter().find(|r| r.is(id))
}

/// Resolves a request's id text (new form, or a bare number) against the library.
pub fn resolve(recs: &[Record], raw: &str) -> Option<GameId> {
    let id = GameId::compat(raw, |n| {
        recs.iter().find(|r| r.legacy() == n).map(|r| r.id.clone())
    })?;
    // An alias answers with the id that replaced it.
    Some(find(recs, &id).map_or(id, |r| r.id.clone()))
}

/// Pids of the emulator processes that were started on this record's file.
pub(crate) fn pids(ps: &[Proc], r: &Record) -> HashSet<u32> {
    let (Some(e), Some(file)) = (registry::get(&r.emulator), r.file.to_str()) else {
        return HashSet::new();
    };
    ps.iter()
        .filter(|p| p.comm == e.cmd && p.argv.iter().any(|a| a == file))
        .map(|p| p.pid)
        .collect()
}

const PLAYED_FILE: &str = "emu-played.json";
const PLAYED_MAX: usize = 500;

/// `{"<game id>": unix seconds}`; a missing or corrupt file is empty.
fn played(data_dir: &Path) -> BTreeMap<String, u64> {
    std::fs::read(data_dir.join(PLAYED_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Atomic 0600 write: a temp file in the same directory, then rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Records now as the game's last-played time: atomic 0600 write, the oldest entries dropped
/// past a cap. A failed write is a warning only.
pub fn mark_played(data_dir: &Path, id: &GameId) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut m = played(data_dir);
    m.insert(id.to_string(), now);
    while m.len() > PLAYED_MAX {
        let oldest = m.iter().min_by_key(|(_, t)| **t).map(|(k, _)| k.clone());
        m.remove(&oldest.unwrap());
    }
    let path = data_dir.join(PLAYED_FILE);
    if let Err(e) = write_atomic(&path, &serde_json::to_vec(&m).unwrap_or_default()) {
        eprintln!("warning: {}: {e}", path.display());
    }
}

/// The library file's records; bad records are skipped with a warning, a corrupt file is empty.
pub fn parse_library(text: &str) -> Result<Vec<Record>, String> {
    #[derive(Deserialize)]
    struct File {
        games: Vec<serde_json::Value>,
    }
    let f: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(f.games
        .into_iter()
        .filter_map(|v| {
            serde_json::from_value::<Record>(v)
                .map_err(|e| e.to_string())
                // the id must be an emu id of the record's own emulator
                .and_then(|r| match r.id.emu_parts() {
                    Some((e, _)) if e == r.emulator => Ok(r),
                    _ => Err(format!(
                        "id {} does not match emulator {}",
                        r.id, r.emulator
                    )),
                })
                .inspect_err(|e| eprintln!("warn: {LIBRARY_FILE}: skipping a game: {e}"))
                .ok()
        })
        .collect())
}

pub fn library_json(recs: &[Record]) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({ "v": 1, "games": recs })).unwrap_or_default()
}

pub fn load(data_dir: &Path) -> Vec<Record> {
    match std::fs::read_to_string(data_dir.join(LIBRARY_FILE)) {
        Ok(text) => parse_library(&text).unwrap_or_else(|e| {
            eprintln!("warn: {LIBRARY_FILE} ignored: {e}");
            vec![]
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let Ok(text) = std::fs::read_to_string(data_dir.join(OLD_FILE)) else {
                return vec![];
            };
            crate::migrate::library_from_emulators(&text).unwrap_or_else(|e| {
                eprintln!("warn: {OLD_FILE} ignored: {e}");
                vec![]
            })
        }
        Err(e) => {
            eprintln!("warn: {LIBRARY_FILE} unreadable: {e}");
            vec![]
        }
    }
}

pub fn caps(emulator: &str) -> Caps {
    Caps {
        controls: registry::get(emulator).is_some_and(|e| !e.controller.is_empty()),
        launch_options: true,
        art: true,
        screenshots: true,
        ..Caps::default()
    }
}

/// One card per record; `installed` means the game file exists, so a missing ISO reads as "not installed".
pub fn games(recs: &[Record], proc: &Path, data_dir: &Path) -> Vec<Game> {
    let ps = procs(proc);
    let played = played(data_dir);
    let pt = crate::playtime::Store::load(data_dir);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let pending = crate::art::load_pending(data_dir);
    recs.iter()
        .map(|r| {
            let mut g = Game::steam(r.legacy(), &r.name);
            g.id = r.id.clone();
            g.source = Source::Emu;
            g.legacy_id = Some(r.legacy().to_string());
            g.caps = caps(&r.emulator);
            // Cheap: no I/O. Dolphin's and Azahar's layouts are known.
            g.caps.saves = (r.emulator == "dolphin"
                && matches!(r.system.as_deref(), Some("wii" | "gamecube")))
                || (r.emulator == "azahar" && crate::saves::program_id(&r.id).is_some());
            g.system = r.system.clone();
            g.installed = r.file.is_file();
            g.size_bytes = std::fs::metadata(&r.file).ok().map(|m| m.len());
            // The launch stamp is the fallback; a later session end wins.
            g.last_played = played
                .get(r.id.as_str())
                .copied()
                .max(pt.last_end(r.id.as_str()));
            g.playtime_min = pt.total_minutes(r.id.as_str());
            g.playtime2wk_min = pt.last_two_weeks_minutes(r.id.as_str(), now);
            g.emulator = Some(r.emulator.clone());
            g.running = !pids(&ps, r).is_empty();
            crate::art::decorate(&mut g, data_dir, &pending);
            g
        })
        .collect()
}

/// Starts the emulator on the game file, detached, in the graphical session, unless it already
/// runs that file: then nothing is spawned. True when the game is running afterwards. False for an
/// unknown id, a missing file, an emulator we don't know, or a failed start. Blocking.
pub fn launch(
    runner: &dyn Runner,
    proc: &Path,
    recs: &[Record],
    id: &GameId,
    env: &[(String, String)],
) -> bool {
    launch_with(runner, proc, recs, id, env, None)
}

/// `launch` with the launch options in `launch_json` (`data_dir/launch.json`) wrapped around the
/// emulator command. Options that are not valid at this moment are not applied (and the log says
/// why): a game always starts.
pub fn launch_with(
    runner: &dyn Runner,
    proc: &Path,
    recs: &[Record],
    id: &GameId,
    env: &[(String, String)],
    launch_json: Option<&Path>,
) -> bool {
    let Some(r) = find(recs, id) else {
        return false;
    };
    let (Some(file), Some(e)) = (
        r.file.to_str().filter(|_| r.file.is_file()),
        registry::get(&r.emulator),
    ) else {
        return false;
    };
    if !pids(&procs(proc), r).is_empty() {
        eprintln!("info: {} is already running; focusing it", r.name);
        return true;
    }
    // The path is a separate argument, never parsed by a shell.
    let base: Vec<String> = std::iter::once(e.cmd)
        .chain(e.args.iter().copied())
        .chain([file])
        .map(String::from)
        .collect();
    let cmd = match launch_json {
        Some(p) => {
            let (c, note) = command_for(&read_file(p), id.as_str(), &base);
            if let Some(n) = note {
                eprintln!("warn: {n}; starting {} without them", r.name);
            }
            c
        }
        None => plain(&base),
    };
    let args: Vec<&str> = cmd.args.iter().map(String::as_str).collect();
    let all: Vec<(String, String)> = env.iter().cloned().chain(cmd.env).collect();
    let wrapped = cmd.program != e.cmd;
    if runner.spawn(&cmd.program, &args, &all) {
        return true;
    }
    // A wrapper that is not installed must not stop the game.
    if wrapped {
        eprintln!(
            "warn: could not start {} through {}; starting it plain",
            r.name, cmd.program
        );
        let args: Vec<&str> = base[1..].iter().map(String::as_str).collect();
        return runner.spawn(e.cmd, &args, env);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

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

    /// A record with the id the migration would give `name`.
    fn rec(name: &str, emu: &str, file: &Path) -> Record {
        Record {
            id: GameId::emu(emu, &crate::migrate::slug(name)).unwrap(),
            emulator: emu.into(),
            system: None,
            file: file.into(),
            name: name.into(),
            added_at: None,
            source: "manual".into(),
            legacy_id: None,
            aliases: vec![],
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
    fn old_ids_stay_computable_and_new_ids_are_separate_from_steam() {
        let r = rec("Mario Kart Wii", "dolphin", Path::new("/x"));
        assert_eq!(r.legacy(), 3907440760);
        assert_eq!(legacy_hash("Mario Kart Wii"), r.legacy());
        assert!(r.legacy() >= crate::game::LEGACY_EMU_BASE);
        assert_eq!(r.id.as_str(), "emu:dolphin:mario-kart-wii");
        // an explicit legacy id (set at migration) wins over the name hash
        let mut r2 = r.clone();
        r2.legacy_id = Some(0xE000_0005);
        assert_eq!(r2.legacy(), 0xE000_0005);
    }

    #[test]
    fn resolve_reads_new_ids_aliases_and_legacy_numbers() {
        let mut r = rec("Mario Kart Wii", "dolphin", Path::new("/x"));
        r.aliases = vec![GameId::parse("emu:dolphin:old-name").unwrap()];
        let recs = [r.clone()];
        let res = |s: &str| resolve(&recs, s).map(|g| g.to_string());
        assert_eq!(
            res("emu:dolphin:mario-kart-wii").as_deref(),
            Some(r.id.as_str())
        );
        assert_eq!(res("emu:dolphin:old-name").as_deref(), Some(r.id.as_str()));
        assert_eq!(res("3907440760").as_deref(), Some(r.id.as_str()));
        assert_eq!(res("620").as_deref(), Some("steam:620"));
        assert_eq!(res("3907440761"), None);
        assert_eq!(res("nope"), None);
    }

    #[test]
    fn games_carry_the_new_fields_and_caps_by_emulator() {
        let f = iso("shape");
        let none = fake_proc(&[]);
        let recs = [rec("MKW", "dolphin", &f), rec("Odd", "mystery", &f)];
        let g = games(&recs, none.path(), none.path());
        assert_eq!(g[0].id.as_str(), "emu:dolphin:mkw");
        assert_eq!(g[0].source, Source::Emu);
        assert_eq!(g[0].appid, legacy_hash("MKW"));
        assert_eq!(
            g[0].legacy_id.as_deref(),
            Some(legacy_hash("MKW").to_string().as_str())
        );
        assert!(g[0].caps.controls && !g[0].caps.install && !g[0].caps.achievements);
        assert!(!g[1].caps.controls, "an emulator without an editor");
        std::fs::remove_file(f).ok();
    }

    #[test]
    fn missing_file_reads_not_installed_and_will_not_launch() {
        let r = rec("Mario Kart Wii", "dolphin", Path::new("/nope/mkwii.iso"));
        let none = fake_proc(&[]);
        assert!(!games(std::slice::from_ref(&r), none.path(), none.path())[0].installed);
        let run = Rec::new(0);
        assert!(!launch(
            &*run,
            none.path(),
            std::slice::from_ref(&r),
            &r.id,
            &[]
        ));
        assert!(run.log().is_empty());
    }

    #[test]
    fn launches_dolphin_on_an_existing_file_only() {
        let f = iso("one");
        let ok = rec("MKW", "dolphin", &f);
        let other = rec("Other", "mystery", &f);
        let none = fake_proc(&[]);
        let run = Rec::new(0);
        let both = [ok.clone(), other.clone()];
        assert!(games(std::slice::from_ref(&ok), none.path(), none.path())[0].installed);
        assert!(launch(&*run, none.path(), &both, &ok.id, &[]));
        assert!(!launch(&*run, none.path(), &both, &other.id, &[]));
        let steam = GameId::steam(730);
        assert!(!launch(
            &*run,
            none.path(),
            std::slice::from_ref(&ok),
            &steam,
            &[]
        ));
        assert_eq!(
            run.log(),
            vec![format!("spawn dolphin-emu -b -e {}", f.display())]
        );
        std::fs::remove_file(f).ok();
    }

    #[test]
    fn azahar_launches_fullscreen_and_is_found_running_by_its_process() {
        let f = iso("az");
        let r = rec("Some 3DS Game", "azahar", &f);
        let run = Rec::new(0);
        let none = tempfile::tempdir().unwrap();
        assert!(launch(
            &*run,
            none.path(),
            std::slice::from_ref(&r),
            &r.id,
            &[]
        ));
        assert_eq!(run.log(), vec![format!("spawn azahar -f {}", f.display())]);
        let sf = f.to_str().unwrap();
        let p = fake_proc(&[(5, "azahar", &["azahar", "-f", sf])]);
        assert_eq!(pids(&procs(p.path()), &r), HashSet::from([5]));
        std::fs::remove_file(f).ok();
    }

    /// Records the whole spawn, env included.
    type Spawn = (String, Vec<String>, Vec<(String, String)>);
    #[derive(Default)]
    struct Spawns(
        Mutex<Vec<Spawn>>,
        Mutex<Vec<String>>, // programs that fail to start
    );
    impl Runner for Spawns {
        fn run(&self, _: &str, _: &[&str]) -> Option<String> {
            None
        }
        fn spawn(&self, p: &str, a: &[&str], e: &[(String, String)]) -> bool {
            let a = a.iter().map(|x| x.to_string()).collect();
            if self.1.lock().unwrap().iter().any(|x| x == p) {
                return false;
            }
            self.0.lock().unwrap().push((p.into(), a, e.to_vec()));
            true
        }
    }

    #[test]
    fn launch_options_wrap_the_emulator_and_invalid_ones_are_dropped() {
        let f = iso("lo");
        let r = rec("MKW", "dolphin", &f);
        let none = fake_proc(&[]);
        let d = tempfile::tempdir().unwrap();
        let lj = d.path().join("launch.json");
        let go = |json: Option<&str>| {
            match json {
                Some(j) => std::fs::write(&lj, j).unwrap(),
                None => drop(std::fs::remove_file(&lj)),
            }
            let run = Spawns::default();
            let env = [("BRISK_X".to_string(), "1".to_string())];
            assert!(launch_with(
                &run,
                none.path(),
                std::slice::from_ref(&r),
                &r.id,
                &env,
                Some(&lj)
            ));
            run.0.lock().unwrap().remove(0)
        };
        let file = f.to_str().unwrap().to_string();
        let ok = r#"{"allowCustomCommands":true,"defaults":{"wrappers":["mangohud","gamemode"],
            "env":{"DXVK_HUD":"fps"},"custom":["mytool"]}}"#;
        let (p, a, e) = go(Some(ok));
        assert_eq!(p, "gamemoderun");
        assert_eq!(
            a,
            [
                "mangohud",
                "mytool",
                "dolphin-emu",
                "-b",
                "-e",
                file.as_str()
            ]
        );
        assert_eq!(e.len(), 2, "caller env and option env");
        // custom commands switched off later: no options at all, and it still starts
        let off = ok.replace("true", "false");
        let (p, a, e) = go(Some(&off));
        assert_eq!(
            (p.as_str(), a),
            (
                "dolphin-emu",
                ["-b", "-e", file.as_str()].map(String::from).to_vec()
            )
        );
        assert_eq!(e, [("BRISK_X".to_string(), "1".to_string())]);
        // a wrapper that is not installed: plain launch as the second spawn
        let run = Spawns::default();
        run.1.lock().unwrap().push("gamemoderun".into());
        std::fs::write(&lj, ok).unwrap();
        assert!(launch_with(
            &run,
            none.path(),
            std::slice::from_ref(&r),
            &r.id,
            &[],
            Some(&lj)
        ));
        assert_eq!(run.0.lock().unwrap()[0].0, "dolphin-emu");
        // no file, corrupt file: plain
        for j in [None, Some("{bad")] {
            assert_eq!(go(j).0, "dolphin-emu");
        }
        std::fs::remove_file(f).ok();
    }

    #[test]
    fn running_means_a_dolphin_process_on_that_file_and_a_second_play_spawns_nothing() {
        let (a, b) = (iso("a"), iso("b"));
        let (ra, rb) = (rec("A", "dolphin", &a), rec("B", "dolphin", &b));
        let (sa, sb) = (a.to_str().unwrap(), b.to_str().unwrap());
        let p = fake_proc(&[
            (10, "dolphin-emu", &["dolphin-emu", "-b", "-e", sa]),
            // Not Dolphin, though it has B's path: must not count.
            (11, "vim", &["vim", sb]),
        ]);
        let rs = [ra.clone(), rb.clone()];
        let g = games(&rs, p.path(), p.path());
        assert_eq!((g[0].running, g[1].running), (true, false));
        let run = Rec::new(0);
        // A is running: Play succeeds without a spawn. B is not: it spawns.
        assert!(launch(&*run, p.path(), &rs, &ra.id, &[]));
        assert!(run.log().is_empty());
        assert!(launch(&*run, p.path(), &rs, &rb.id, &[]));
        assert_eq!(run.log(), vec![format!("spawn dolphin-emu -b -e {sb}")]);
        std::fs::remove_file(a).ok();
        std::fs::remove_file(b).ok();
    }

    /// The focus loop is shared (`stream::focus` picks the class from the source).
    #[tokio::test(start_paused = true)]
    async fn focus_polls_until_the_window_appears_then_focuses_and_fullscreens() {
        let d = tempfile::tempdir().unwrap();
        let r = rec("MKW", "dolphin", Path::new("/x.iso"));
        std::fs::write(
            d.path().join(LIBRARY_FILE),
            library_json(std::slice::from_ref(&r)),
        )
        .unwrap();
        let run = Rec::new(4);
        let env = vec![("SWAYSOCK".to_string(), "/s".to_string())];
        crate::stream::focus(
            run.clone(),
            PathBuf::from("/nonexistent-proc"),
            d.path().into(),
            env,
            r.id.clone(),
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(*run.trees.lock().unwrap(), 4);
        assert_eq!(
            run.log(),
            vec!["swaymsg -s /s [con_id=40] focus, fullscreen enable"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn focus_gives_up_cleanly_when_no_window_ever_appears() {
        let d = tempfile::tempdir().unwrap();
        let r = rec("MKW", "dolphin", Path::new("/x.iso"));
        std::fs::write(
            d.path().join(LIBRARY_FILE),
            library_json(std::slice::from_ref(&r)),
        )
        .unwrap();
        let run = Rec::new(u32::MAX);
        crate::stream::focus(
            run.clone(),
            PathBuf::from("/nonexistent-proc"),
            d.path().into(),
            vec![],
            r.id.clone(),
            Duration::from_secs(60),
        )
        .await;
        assert!(run.log().is_empty(), "nothing is focused: {:?}", run.log());
        assert!(*run.trees.lock().unwrap() >= 60);
    }

    #[test]
    fn mark_played_feeds_games_and_a_corrupt_file_is_tolerated() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let r = rec("MKW", "dolphin", Path::new("/x.iso"));
        let none = fake_proc(&[]);
        let g = |d: &Path| games(std::slice::from_ref(&r), none.path(), d).remove(0);
        assert_eq!(g(d.path()).last_played, None);
        std::fs::write(d.path().join(PLAYED_FILE), "{not json").unwrap();
        assert_eq!(g(d.path()).last_played, None);
        mark_played(d.path(), &r.id);
        assert!(g(d.path()).last_played.unwrap() > 1_700_000_000);
        let mode = std::fs::metadata(d.path().join(PLAYED_FILE)).unwrap();
        assert_eq!(mode.permissions().mode() & 0o777, 0o600);
        assert_eq!(g(d.path()).emulator.as_deref(), Some("dolphin"));
    }

    #[test]
    fn games_show_playtime_and_the_later_of_launch_and_session_end() {
        let d = tempfile::tempdir().unwrap();
        let r = rec("MKW", "dolphin", Path::new("/x.iso"));
        let none = fake_proc(&[]);
        let g = |d: &Path| games(std::slice::from_ref(&r), none.path(), d).remove(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut pt = crate::playtime::Store::default();
        pt.add(r.id.as_str(), now - 7200, now - 3600);
        pt.save(d.path());
        let x = g(d.path());
        assert_eq!((x.playtime_min, x.playtime2wk_min), (60, 60));
        assert_eq!(
            x.last_played,
            Some(now - 3600),
            "no launch stamp: session end"
        );
        std::fs::write(
            d.path().join(PLAYED_FILE),
            format!(r#"{{"{}":{}}}"#, r.id, now - 100),
        )
        .unwrap();
        assert_eq!(
            g(d.path()).last_played,
            Some(now - 100),
            "later launch stamp wins"
        );
    }

    #[test]
    fn library_file_wins_and_the_old_file_is_the_fallback_and_corrupt_is_empty() {
        let d = tempfile::tempdir().unwrap();
        assert!(load(d.path()).is_empty());
        std::fs::write(d.path().join(OLD_FILE), "{not json").unwrap();
        assert!(load(d.path()).is_empty());
        std::fs::write(
            d.path().join(OLD_FILE),
            r#"[{"name":"A","emulator":"dolphin","file":"/a.iso"}]"#,
        )
        .unwrap();
        assert_eq!(load(d.path())[0].id.as_str(), "emu:dolphin:a");
        // library.json present: the old file is no longer read
        let b = rec("B", "dolphin", Path::new("/b.iso"));
        std::fs::write(d.path().join(LIBRARY_FILE), library_json(&[b])).unwrap();
        let l = load(d.path());
        assert_eq!((l.len(), l[0].id.as_str()), (1, "emu:dolphin:b"));
        std::fs::write(d.path().join(LIBRARY_FILE), "{not json").unwrap();
        assert!(load(d.path()).is_empty());
    }

    #[test]
    fn a_bad_record_is_skipped_not_fatal() {
        let t = r#"{"v":1,"games":[
            {"id":"emu:dolphin:ok","emulator":"dolphin","file":"/a","name":"A"},
            {"id":"steam:5","emulator":"dolphin","file":"/b","name":"B"},
            {"id":"emu:dolphin:ok2","emulator":"dolphin","file":"/c","name":"C","future":{"x":1}}]}"#;
        let l = parse_library(t).unwrap();
        assert_eq!(
            l.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["A", "C"]
        );
    }
}
