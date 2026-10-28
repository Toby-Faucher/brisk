//! Getting game files into the library: the games-folder scan, the library file changes (add,
//! forget) and `brisk-hostd add`. Design: docs/plans/2026-10-28-stage2-upload.md.
//! A scan never loads a file: `discid::identify` reads a header, and hashes a cartridge by streaming.

use crate::config::Config;
use crate::discid::{self, Identity, System};
use crate::emu::{self, Record};
use crate::files;
use crate::game::GameId;
use crate::registry;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Extensions a scan or an upload looks at.
pub const EXTS: &[&str] = &[
    "iso", "gcm", "wbfs", "rvz", "wia", "nes", "sfc", "smc", "gb", "gbc", "gba", "md", "gen",
];
const MAX_DEPTH: usize = 3;
const MAX_ENTRIES: usize = 20_000;

pub fn known_ext(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The games-folder sub-folder, which is also the registry's system id where one exists.
pub fn folder(s: System) -> &'static str {
    match s {
        System::Wii => "wii",
        System::GameCube => "gamecube",
        System::NES => "nes",
        System::SNES => "snes",
        System::MegaDrive => "megadrive",
        System::GB => "gb",
        System::GBC => "gbc",
        System::GBA => "gba",
        System::Unknown => "unknown",
    }
}

pub fn emulator_for(s: System) -> Option<&'static registry::Emulator> {
    registry::EMULATORS
        .iter()
        .find(|e| e.systems.iter().any(|x| x.id == folder(s)))
}

pub struct Found {
    pub path: PathBuf,
    pub id: Identity,
}

#[derive(Default)]
pub struct Scan {
    pub found: Vec<Found>,
    /// Files with a known extension that were looked at.
    pub scanned: usize,
    pub unknown: usize,
    /// A known format with a damaged header, or unreadable.
    pub broken: usize,
    /// The entry cap stopped the walk.
    pub truncated: bool,
}

struct Walk {
    files: Vec<PathBuf>,
    seen: usize,
    truncated: bool,
}

fn walk(dir: &Path, depth: usize, w: &mut Walk) {
    let mut es: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    es.sort_by_key(|e| e.file_name());
    for e in es {
        if w.seen >= MAX_ENTRIES {
            w.truncated = true;
            return;
        }
        w.seen += 1;
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue; // dot-dirs (.uploads) and hidden files
        }
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        // A symlinked folder directly under the root is the owner's choice (~/Games/wii -> ~/roms);
        // anywhere else symlinks are not followed.
        let dir_like = ft.is_dir() || (ft.is_symlink() && depth == 0 && p.is_dir());
        if dir_like {
            if depth < MAX_DEPTH {
                walk(&p, depth + 1, w);
            }
        } else if ft.is_file() && known_ext(&name) {
            w.files.push(p);
        }
    }
}

/// Identifies the game files under `root`. A missing root is an empty scan.
pub fn scan(root: &Path) -> Scan {
    let mut w = Walk {
        files: vec![],
        seen: 0,
        truncated: false,
    };
    walk(root, 0, &mut w);
    let mut s = Scan {
        truncated: w.truncated,
        scanned: w.files.len(),
        ..Scan::default()
    };
    for path in w.files {
        match discid::identify(&path) {
            Ok(id) if id.system == System::Unknown => s.unknown += 1,
            Ok(id) => s.found.push(Found { path, id }),
            Err(_) => s.broken += 1,
        }
    }
    s
}

/// The stable part of the game id. Disc games: the 6-character disc ID. Cartridges: the first 16
/// hex of discid's plain SHA-1, which is NOT the RetroAchievements hash.
pub fn key(i: &Identity) -> Option<String> {
    match (&i.game_id, &i.hash) {
        (Some(g), _) if matches!(i.system, System::Wii | System::GameCube) => Some(g.clone()),
        (_, Some(h)) => Some(h.chars().take(16).collect()),
        _ => None,
    }
}

/// The library record for an identified file; None when no emulator plays that system.
pub fn record(path: &Path, i: &Identity, source: &str) -> Option<Record> {
    let e = emulator_for(i.system)?;
    let id = GameId::emu(e.id, &key(i)?)?;
    let stem = || {
        path.file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
    };
    let name = i
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(stem);
    Some(Record {
        id,
        emulator: e.id.into(),
        system: Some(folder(i.system).into()),
        file: path.to_path_buf(),
        name,
        added_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs()),
        source: source.into(),
        legacy_id: None,
        aliases: vec![],
    })
}

#[derive(Default, Debug)]
pub struct Merged {
    pub added: Vec<GameId>,
    pub aliased: usize,
    pub duplicates: usize,
    pub unplayable: usize,
    /// Skipped because the owner removed them from the library.
    pub removed: usize,
}

/// Canonical paths, each looked up once (a scan compares every record with every file).
struct Canon<'a> {
    f: &'a dyn Fn(&Path) -> Option<PathBuf>,
    seen: HashMap<PathBuf, Option<Rc<Path>>>,
}

impl<'a> Canon<'a> {
    fn new(f: &'a dyn Fn(&Path) -> Option<PathBuf>) -> Self {
        Canon {
            f,
            seen: HashMap::new(),
        }
    }
    fn get(&mut self, p: &Path) -> Option<Rc<Path>> {
        if let Some(c) = self.seen.get(p) {
            return c.clone();
        }
        let c = (self.f)(p).map(Rc::from);
        self.seen.insert(p.to_path_buf(), c.clone());
        c
    }
}

fn real_canon(p: &Path) -> Option<PathBuf> {
    p.canonicalize().ok()
}

/// Adds what is new. Same path under another id: that record keeps its id and gains the new one
/// as an alias. Same id (or alias) at another path: a duplicate, not added.
pub fn merge(recs: &mut Vec<Record>, found: &[Found], source: &str) -> Merged {
    merge_with(recs, found, source, &mut Canon::new(&real_canon))
}

fn merge_with(recs: &mut Vec<Record>, found: &[Found], source: &str, c: &mut Canon) -> Merged {
    let mut m = Merged::default();
    // Lookups by id and by path (as written and as resolved), built once: the first record wins.
    #[derive(Default)]
    struct Index {
        ids: HashSet<String>,
        raw: HashMap<PathBuf, usize>,
        real: HashMap<Rc<Path>, usize>,
    }
    impl Index {
        fn add(&mut self, i: usize, r: &Record, c: &mut Canon) {
            self.ids.insert(r.id.to_string());
            self.ids.extend(r.aliases.iter().map(|a| a.to_string()));
            self.raw.entry(r.file.clone()).or_insert(i);
            if let Some(x) = c.get(&r.file) {
                self.real.entry(x).or_insert(i);
            }
        }
    }
    let mut ix = Index::default();
    for (i, r) in recs.iter().enumerate() {
        ix.add(i, r, c);
    }
    for f in found {
        let Some(new) = record(&f.path, &f.id, source) else {
            m.unplayable += 1;
            continue;
        };
        let id_taken = ix.ids.contains(&new.id.to_string());
        let hit = ix
            .raw
            .get(&new.file)
            .or_else(|| c.get(&new.file).and_then(|x| ix.real.get(&x)))
            .copied();
        if let Some(i) = hit {
            let r = &mut recs[i];
            if !id_taken {
                ix.ids.insert(new.id.to_string());
                r.aliases.push(new.id);
                m.aliased += 1;
            } else {
                m.duplicates += 1;
            }
            if r.system.is_none() {
                r.system = new.system;
            }
        } else if id_taken {
            m.duplicates += 1;
        } else {
            m.added.push(new.id.clone());
            ix.add(recs.len(), &new, c);
            recs.push(new);
        }
    }
    m
}

const MAX_TOMBS: usize = 5000;
const TOMBS_FILE: &str = "removed.json";

/// A game the owner removed: its file stays on disk, so a scan must not bring it back.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Tomb {
    pub path: PathBuf,
    pub id: String,
}

/// The removed list; a missing or damaged file is an empty list.
pub fn tombs_load(data_dir: &Path) -> Vec<Tomb> {
    #[derive(serde::Deserialize)]
    struct F {
        removed: Vec<Tomb>,
    }
    std::fs::read_to_string(data_dir.join(TOMBS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<F>(&t).ok())
        .map_or_else(Vec::new, |f| f.removed)
}

/// Atomic 0600 write, the oldest entries dropped past the cap. A failed write is a warning only.
pub fn tombs_save(data_dir: &Path, mut t: Vec<Tomb>) {
    t.drain(..t.len().saturating_sub(MAX_TOMBS));
    let b = serde_json::to_vec(&serde_json::json!({ "removed": t })).unwrap_or_default();
    if let Err(e) = emu::write_atomic(&data_dir.join(TOMBS_FILE), &b) {
        eprintln!("warning: removed list: {e}");
    }
}

/// What a file or a removed entry can be recognised by: its path, its real path, its game id.
#[derive(Default)]
struct Keys {
    raw: HashSet<PathBuf>,
    real: HashSet<Rc<Path>>,
    ids: HashSet<String>,
}

impl Keys {
    fn add(&mut self, path: &Path, id: Option<String>, c: &mut Canon) {
        self.raw.insert(path.to_path_buf());
        self.real.extend(c.get(path));
        self.ids.extend(id);
    }
    fn hits(&self, path: &Path, id: Option<String>, c: &mut Canon) -> bool {
        self.raw.contains(path)
            || c.get(path).is_some_and(|r| self.real.contains(&r))
            || id.is_some_and(|i| self.ids.contains(&i))
    }
}

fn found_id(f: &Found, source: &str) -> Option<String> {
    record(&f.path, &f.id, source).map(|r| r.id.to_string())
}

/// `merge` with the removed list (call under `update`). `respect`: a scan skips removed games
/// and counts them; an explicit add (`respect` false) clears the entry instead. Whatever gets
/// registered is cleared from the list.
pub fn register(
    data_dir: &Path,
    recs: &mut Vec<Record>,
    found: Vec<Found>,
    source: &str,
    respect: bool,
) -> Merged {
    register_with(
        data_dir,
        recs,
        found,
        source,
        respect,
        &mut Canon::new(&real_canon),
    )
}

fn register_with(
    data_dir: &Path,
    recs: &mut Vec<Record>,
    found: Vec<Found>,
    source: &str,
    respect: bool,
    c: &mut Canon,
) -> Merged {
    let mut tombs = tombs_load(data_dir);
    let mut gone = Keys::default();
    for t in &tombs {
        gone.add(&t.path, Some(t.id.clone()), c);
    }
    let (skip, keep): (Vec<_>, Vec<_>) = found
        .into_iter()
        .partition(|f| respect && gone.hits(&f.path, found_id(f, source), c));
    let mut m = merge_with(recs, &keep, source, c);
    m.removed = skip.len();
    let n = tombs.len();
    let mut kept = Keys::default();
    for f in &keep {
        kept.add(&f.path, found_id(f, source), c);
    }
    tombs.retain(|t| !kept.hits(&t.path, Some(t.id.clone()), c));
    if tombs.len() != n {
        tombs_save(data_dir, tombs);
    }
    m
}

/// Exclusive lock on `data_dir/library.lock`, shared with other hostd processes (`add`).
pub struct Lock(File);

pub fn lock(data_dir: &Path) -> io::Result<Lock> {
    std::fs::create_dir_all(data_dir)?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(data_dir.join("library.lock"))?;
    loop {
        // SAFETY: the fd is open for the whole call; flock only reads it.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(Lock(f));
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // SAFETY: as in `lock`; closing the file would release it too.
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// The records, but a `library.json` that does not parse is an error: it must never be overwritten.
pub fn load_strict(data_dir: &Path) -> Result<Vec<Record>, String> {
    match std::fs::read_to_string(data_dir.join(emu::LIBRARY_FILE)) {
        Ok(t) => emu::parse_library(&t).map_err(|e| format!("library.json is damaged: {e}")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(emu::load(data_dir)),
        Err(e) => Err(format!("library.json unreadable: {e}")),
    }
}

/// Load, change, save atomically, all under the lock. `f` returns whether it changed anything.
pub fn update<R>(
    data_dir: &Path,
    f: impl FnOnce(&mut Vec<Record>) -> (bool, R),
) -> Result<R, String> {
    let _l = lock(data_dir).map_err(|e| format!("library lock: {e}"))?;
    let mut recs = load_strict(data_dir)?;
    let (changed, r) = f(&mut recs);
    if changed {
        let p = data_dir.join(emu::LIBRARY_FILE);
        emu::write_atomic(&p, &emu::library_json(&recs))
            .map_err(|e| format!("library save: {e}"))?;
    }
    Ok(r)
}

/// Forgets the record `id` names (the file stays). False when there was none.
pub fn forget(data_dir: &Path, id: &GameId) -> Result<bool, String> {
    update(data_dir, |recs| {
        let gone: Vec<_> = recs.iter().filter(|r| r.is(id)).cloned().collect();
        recs.retain(|r| !r.is(id));
        if !gone.is_empty() {
            let mut t = tombs_load(data_dir);
            t.extend(gone.iter().map(|r| Tomb {
                path: r.file.canonicalize().unwrap_or_else(|_| r.file.clone()),
                id: r.id.to_string(),
            }));
            tombs_save(data_dir, t);
        }
        (!gone.is_empty(), !gone.is_empty())
    })
}

pub struct ScanReport {
    pub scan: Scan,
    pub merged: Merged,
}

/// Scans `cfg.games_dir` and registers what is new. The walk runs outside the lock.
pub fn scan_library(cfg: &Config) -> Result<ScanReport, String> {
    scan_library_with(cfg, false)
}

/// `include_removed` ignores the removed list, so those games come back.
pub fn scan_library_with(cfg: &Config, include_removed: bool) -> Result<ScanReport, String> {
    let mut scan = scan(&cfg.games_dir);
    let found = std::mem::take(&mut scan.found);
    let merged = update(&cfg.data_dir, |recs| {
        let m = register(&cfg.data_dir, recs, found, "scan", !include_removed);
        (!m.added.is_empty() || m.aliased > 0, m)
    })?;
    Ok(ScanReport { scan, merged })
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Move,
    Copy,
    Link,
}

fn copy_new(src: &Path, dst: &Path) -> io::Result<()> {
    let mut to = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(dst)?;
    io::copy(&mut File::open(src)?, &mut to)
        .and_then(|_| to.sync_all())
        .inspect_err(|_| {
            let _ = std::fs::remove_file(dst);
        })
        .map(|_| ())
}

/// Puts `src` at `dir/name` (a free ` (n)` name on a clash, never overwriting). Move uses a hard
/// link and then unlinks `src` (a copy when they are on different disks).
pub fn place(src: &Path, dir: &Path, name: &str, mode: Mode) -> io::Result<PathBuf> {
    place_with(src, dir, name, mode, &|a, b| std::fs::hard_link(a, b))
}

/// `place` with the hard-link call injectable (exFAT, FAT and some network mounts refuse it).
fn place_with(
    src: &Path,
    dir: &Path,
    name: &str,
    mode: Mode,
    link: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    for i in 0..10_000 {
        let p = dir.join(files::candidate(name, i));
        let r = match mode {
            Mode::Link => std::os::unix::fs::symlink(src, &p),
            Mode::Copy => copy_new(src, &p),
            // Any link failure but a name clash falls back to a copy (the source is removed below).
            Mode::Move => link(src, &p).or_else(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    Err(e)
                } else {
                    copy_new(src, &p)
                }
            }),
        };
        match r {
            Ok(()) => {
                if mode == Mode::Move {
                    std::fs::remove_file(src)?;
                }
                return Ok(p);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many files with this name"))
}

/// `brisk-hostd add <file> [--move|--copy|--link]`: (exit code, message).
pub fn add_cmd(cfg: &Config, args: &[String]) -> (i32, String) {
    const USAGE: &str = "usage: brisk-hostd add <file> [--move|--copy|--link]";
    let (mut file, mut mode) = (None, None);
    for a in args {
        let m = match a.as_str() {
            "--move" => Some(Mode::Move),
            "--copy" => Some(Mode::Copy),
            "--link" => Some(Mode::Link),
            s if s.starts_with("--") => return (2, USAGE.into()),
            _ => None,
        };
        if m.is_some() && mode.is_none() {
            mode = m;
        } else if m.is_none() && file.is_none() {
            file = Some(a);
        } else {
            return (2, USAGE.into());
        }
    }
    let Some(file) = file else {
        return (2, USAGE.into());
    };
    let src = match Path::new(file).canonicalize() {
        Ok(p) => p,
        Err(e) => return (1, format!("{file}: {e}")),
    };
    let id = match discid::identify(&src) {
        Ok(i) => i,
        Err(e) => return (1, format!("{file}: {e}")),
    };
    if id.system == System::Unknown {
        return (1, format!("{file}: not a game file brisk recognises"));
    }
    if emulator_for(id.system).is_none() {
        return (
            1,
            format!("{file}: no emulator for {} yet", folder(id.system)),
        );
    }
    let path = match mode {
        None => src,
        Some(m) => {
            let name = src
                .file_name()
                .and_then(|n| files::sanitize(&n.to_string_lossy()))
                .unwrap_or_else(|| "game".into());
            match place(&src, &cfg.games_dir.join(folder(id.system)), &name, m) {
                Ok(p) => p,
                Err(e) => return (1, format!("{file}: {e}")),
            }
        }
    };
    let f = vec![Found { path, id }];
    match update(&cfg.data_dir, |recs| {
        let m = register(&cfg.data_dir, recs, f, "add", false);
        (!m.added.is_empty() || m.aliased > 0, m)
    }) {
        Ok(m) if !m.added.is_empty() => (0, format!("added {}", m.added[0])),
        Ok(m) if m.aliased > 0 => (0, "already in the library (new id kept as an alias)".into()),
        Ok(_) => (0, "already in the library".into()),
        Err(e) => (1, e),
    }
}

#[cfg(test)]
mod tests;
