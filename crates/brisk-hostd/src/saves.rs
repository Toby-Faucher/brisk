//! Read-only list of an emulator game's saves, from Dolphin's user directory or a melonDS game's
//! folders. Only names, sizes and times are read, never save contents; paths are relative to the user
//! dir.
//!
//! Layout, checked against Dolphin's source (CommonPaths.h, State.cpp, UICommon.cpp, NandPaths.cpp,
//! MainSettings.cpp, Formats.h):
//! - save states: `StateSaves/<GameID>.sNN` (`{}.s{:02d}`, NN from 01), `<GameID>` is the 6-char disc id.
//! - Wii saves: `Wii/title/<hi:08x>/<lo:08x>/data`, lo = the first 4 chars of the disc id as hex,
//!   hi = 00010000 (Game) or 00010004 (GameWithChannel).
//! - GameCube cards: `GC/MemoryCard{A,B}.<REGION>[.<blocks>].raw` and `GC/<REGION>/Card A` (GCI folder).
//!
//! shortcut: the 10-slot count (NUM_STATES) and the region strings (USA, EUR, JPN/JAP) were not seen in
//! the source text; they are from memory. Re-check against a real user dir.
//! shortcut: `lastState.sav` (the undo state, shared by all games) is not listed.
//! shortcut: symlinks anywhere under the user dir are skipped, even ones that point inside it.
//!
//! melonDS (checked against its 1.1 source, EmuInstance.cpp `getAssetPath`, `getSavestateName`,
//! `loadROM`; Window.cpp; Config.cpp): the game save is `<rom stem>.sav` and the state slots 1 to 8
//! are `<rom stem>.ml1` to `.ml8`, all next to the ROM. `[Instance0] SaveFilePath` and `SavestatePath`
//! in `melonDS.toml` move them to another folder. Paths are then the bare file names.
//! shortcut: states saved with "File..." (any name, `.mln` by default), the `.sav` that
//! `Savestate.RelocSRAM` writes next to a state, and the `.2`+ suffixed files of a second instance are
//! not listed.

use serde::Serialize;
use std::fs::Metadata;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_ITEMS: usize = 100;
const MAX_WALK: usize = 2000;
const MAX_DEPTH: usize = 4;
const STATE_SLOTS: u32 = 10;
const MELON_SLOTS: u32 = 8;
/// More than any real `melonDS.toml`; the rest is not read.
const MELON_TOML_MAX: u64 = 256 * 1024;

/// The kind of save data.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    State,
    MemoryCard,
    NandSave,
    /// A cartridge save (melonDS `.sav`).
    GameSave,
}

/// One save file or folder an emulator holds.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveItem {
    pub kind: Kind,
    pub name: String,
    /// Relative to the emulator user dir, no `..`.
    pub path: String,
    pub size: u64,
    /// Unix seconds.
    pub modified: Option<u64>,
    /// Files inside (Wii save folders only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The 6-char disc id inside an emulator game id (`emu:dolphin:RMCE01`, any case), if it has one.
pub(crate) fn disc_id(id: &crate::game::GameId) -> Option<String> {
    let (_, tail) = id.emu_parts()?;
    (tail.len() == 6 && tail.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| tail.to_ascii_uppercase())
}

fn secs(m: &Metadata) -> Option<u64> {
    m.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// The metadata of `p` if it is a real (not symlinked) entry.
fn plain(p: &Path) -> Option<Metadata> {
    std::fs::symlink_metadata(p)
        .ok()
        .filter(|m| !m.is_symlink())
}

/// `base/parts...` if every step is a real directory.
fn dir_under(base: &Path, parts: &[&str]) -> Option<PathBuf> {
    let mut p = base.to_path_buf();
    for part in parts {
        p.push(part);
        if !plain(&p)?.is_dir() {
            return None;
        }
    }
    Some(p)
}

fn rel(parts: &[&str]) -> String {
    parts.join("/")
}

fn file_item(base: &Path, parts: &[&str], kind: Kind, note: Option<&str>) -> Option<SaveItem> {
    let m = plain(&base.join(parts.join("/")))?;
    m.is_file().then(|| SaveItem {
        kind,
        name: parts[parts.len() - 1].to_string(),
        path: rel(parts),
        size: m.len(),
        modified: secs(&m),
        files: None,
        note: note.map(Into::into),
    })
}

/// What `walk` found under a folder.
struct Tally {
    files: u32,
    size: u64,
    newest: Option<u64>,
}

/// Regular files under `dir`: count, total size, newest mtime. Bounded by entries and depth.
fn walk(dir: &Path) -> Tally {
    let (mut n, mut size, mut newest) = (0u32, 0u64, None);
    let mut seen = 0;
    let mut todo = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen > MAX_WALK {
                return Tally {
                    files: n,
                    size,
                    newest,
                };
            }
            let Some(m) = plain(&e.path()) else { continue };
            if m.is_file() {
                n += 1;
                size += m.len();
                newest = newest.max(secs(&m));
            } else if m.is_dir() && depth < MAX_DEPTH {
                todo.push((e.path(), depth + 1));
            }
        }
    }
    Tally {
        files: n,
        size,
        newest,
    }
}

/// Cards whose name says they belong to the game's region (all cards when the region is unknown).
fn region_tags(disc: &str) -> &'static [&'static str] {
    match disc.as_bytes().get(3) {
        Some(b'E') => &["USA"],
        Some(b'J') => &["JPN", "JAP"],
        Some(b'P') => &["EUR"],
        _ => &[],
    }
}

/// The saves of one game; `None` when the user dir does not exist. Newest first.
pub fn list(user: &Path, disc: &str, system: &str) -> Option<Vec<SaveItem>> {
    plain(user).or_else(|| std::fs::metadata(user).ok())?;
    let mut out = vec![];
    if disc.len() != 6 || !disc.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Some(out);
    }
    if dir_under(user, &["StateSaves"]).is_some() {
        for n in 1..=STATE_SLOTS {
            let name = format!("{disc}.s{n:02}");
            out.extend(file_item(user, &["StateSaves", &name], Kind::State, None));
        }
    }
    if system == "wii" {
        let lo: String = disc.bytes().take(4).map(|b| format!("{b:02x}")).collect();
        for hi in ["00010000", "00010004"] {
            let parts = ["Wii", "title", hi, &lo, "data"];
            let Some(dir) = dir_under(user, &parts) else {
                continue;
            };
            let Tally {
                files,
                size,
                newest: modified,
            } = walk(&dir);
            out.push(SaveItem {
                kind: Kind::NandSave,
                name: "Wii save data".into(),
                path: rel(&parts),
                size,
                modified,
                files: Some(files),
                note: None,
            });
        }
    } else if let Some(gc) = dir_under(user, &["GC"]) {
        let tags = region_tags(disc);
        let mut names: Vec<String> = std::fs::read_dir(gc)
            .into_iter()
            .flatten()
            .flatten()
            .take(MAX_WALK)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| {
                n.ends_with(".raw")
                    && (n.starts_with("MemoryCardA.") || n.starts_with("MemoryCardB."))
                    && (tags.is_empty() || tags.iter().any(|t| n.split('.').nth(1) == Some(t)))
            })
            .collect();
        names.sort();
        for n in names {
            out.extend(file_item(
                user,
                &["GC", &n],
                Kind::MemoryCard,
                Some("A memory card is shared by every game of its region"),
            ));
        }
    }
    Some(newest_first(out))
}

fn newest_first(mut v: Vec<SaveItem>) -> Vec<SaveItem> {
    v.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    v.truncate(MAX_ITEMS);
    v
}

/// An absolute folder set as `key` in the `[Instance0]` table of `melonDS.toml`.
/// shortcut: a line scan, not a TOML parser. Values with escapes, relative paths, dotted keys and
/// inline tables are ignored (the ROM folder is used); fine for the file melonDS writes itself.
fn melon_dir(toml: &str, key: &str) -> Option<PathBuf> {
    let mut table = "";
    for line in toml.lines().map(str::trim) {
        if let Some(h) = line.strip_prefix('[') {
            table = h.trim_end_matches(']').trim();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if table != "Instance0" || k.trim() != key {
            continue;
        }
        let v = v.trim();
        let s = (v.strip_prefix('"').and_then(|v| v.strip_suffix('"')))
            .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))?;
        let p = PathBuf::from(s);
        return (!s.contains('\\') && p.is_absolute()).then_some(p);
    }
    None
}

/// The game save and save states of a melonDS game, found from its ROM path; `None` when the ROM's
/// folder does not exist. `cfg` is melonDS's config dir. Newest first.
pub fn list_melonds(rom: &Path, cfg: &Path) -> Option<Vec<SaveItem>> {
    let home = rom.parent().filter(|d| d.is_dir())?;
    // melonDS cuts the name at its last dot, as file_stem does.
    let Some(stem) = rom.file_stem().and_then(|s| s.to_str()) else {
        return Some(vec![]);
    };
    let mut toml = String::new();
    let f = cfg.join("melonDS.toml");
    if plain(&f).is_some_and(|m| m.is_file())
        && let Ok(f) = std::fs::File::open(&f)
    {
        // Not UTF-8 or unreadable: no redirects.
        let _ = f.take(MELON_TOML_MAX).read_to_string(&mut toml);
    }
    let dir = |key| melon_dir(&toml, key).unwrap_or_else(|| home.to_path_buf());
    let (saves, states) = (dir("SaveFilePath"), dir("SavestatePath"));
    let mut out: Vec<SaveItem> = file_item(&saves, &[&format!("{stem}.sav")], Kind::GameSave, None)
        .into_iter()
        .collect();
    for n in 1..=MELON_SLOTS {
        out.extend(file_item(
            &states,
            &[&format!("{stem}.ml{n}")],
            Kind::State,
            None,
        ));
    }
    Some(newest_first(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn put(root: &Path, rel: &str, len: usize) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![0u8; len]).unwrap();
    }

    fn tree() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        put(r, "StateSaves/RMCE01.s01", 10);
        put(r, "StateSaves/RMCE01.s10", 20);
        put(r, "StateSaves/RMCE01.s11", 5);
        put(r, "StateSaves/RMCE01.s01.dtm", 5);
        put(r, "StateSaves/OTHER1.s01", 5);
        // RMCE -> 52 4d 43 45
        put(r, "Wii/title/00010000/524d4345/data/a.bin", 100);
        put(r, "Wii/title/00010000/524d4345/data/sub/b.bin", 50);
        put(r, "GC/MemoryCardA.USA.raw", 1000);
        put(r, "GC/MemoryCardA.EUR.raw", 1000);
        put(r, "GC/MemoryCardB.USA.raw", 1000);
        t
    }

    #[test]
    fn disc_id_from_game_id() {
        let g = |s: &str| crate::game::GameId::parse(s).unwrap();
        assert_eq!(disc_id(&g("emu:dolphin:rmce01")).as_deref(), Some("RMCE01"));
        assert_eq!(disc_id(&g("emu:dolphin:mario-kart-wii")), None);
        assert_eq!(disc_id(&g("emu:dolphin:rmce0")), None);
    }

    #[test]
    fn lists_states_and_wii_nand_save() {
        let t = tree();
        let v = list(t.path(), "RMCE01", "wii").unwrap();
        let paths: Vec<_> = v.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"StateSaves/RMCE01.s01"));
        assert!(paths.contains(&"StateSaves/RMCE01.s10"));
        assert_eq!(v.iter().filter(|i| i.kind == Kind::State).count(), 2);
        let n = v.iter().find(|i| i.kind == Kind::NandSave).unwrap();
        assert_eq!(n.path, "Wii/title/00010000/524d4345/data");
        assert_eq!((n.files, n.size), (Some(2), 150));
        assert!(v.iter().all(|i| i.kind != Kind::MemoryCard));
        assert!(
            v.iter()
                .all(|i| !i.path.starts_with('/') && !i.path.contains(".."))
        );
    }

    #[test]
    fn gamecube_lists_the_regions_cards_with_a_note() {
        let t = tree();
        let v = list(t.path(), "GALE01", "gamecube").unwrap();
        let cards: Vec<_> = v.iter().filter(|i| i.kind == Kind::MemoryCard).collect();
        assert_eq!(cards.len(), 2);
        assert!(
            cards
                .iter()
                .all(|c| c.path.contains(".USA.") && c.note.is_some())
        );
        assert!(v.iter().all(|i| i.kind != Kind::NandSave));
    }

    #[test]
    fn no_user_dir_is_none_and_no_saves_is_empty() {
        let t = tempfile::tempdir().unwrap();
        assert!(list(&t.path().join("missing"), "RMCE01", "wii").is_none());
        assert_eq!(list(t.path(), "RMCE01", "wii").unwrap(), vec![]);
    }

    #[test]
    fn newest_first() {
        let t = tree();
        let f = fs::File::options()
            .write(true)
            .open(t.path().join("StateSaves/RMCE01.s10"))
            .unwrap();
        f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(5))
            .unwrap();
        let v = list(t.path(), "RMCE01", "wii").unwrap();
        let m: Vec<_> = v.iter().map(|i| i.modified).collect();
        assert!(m.windows(2).all(|w| w[0] >= w[1]));
        assert_eq!(v.last().unwrap().path, "StateSaves/RMCE01.s10");
    }

    #[test]
    fn hostile_entries_are_skipped() {
        let t = tree();
        let out = tempfile::tempdir().unwrap();
        put(out.path(), "secret.bin", 1);
        let r = t.path();
        // a state slot, a card and the NAND dir symlinked out; a folder in place of a state file
        fs::remove_file(r.join("StateSaves/RMCE01.s01")).unwrap();
        std::os::unix::fs::symlink(
            out.path().join("secret.bin"),
            r.join("StateSaves/RMCE01.s01"),
        )
        .unwrap();
        fs::create_dir(r.join("StateSaves/RMCE01.s02")).unwrap();
        fs::remove_dir_all(r.join("Wii/title/00010000/524d4345/data")).unwrap();
        std::os::unix::fs::symlink(out.path(), r.join("Wii/title/00010000/524d4345/data")).unwrap();
        let v = list(r, "RMCE01", "wii").unwrap();
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].path, "StateSaves/RMCE01.s10");
        // a symlinked StateSaves dir is not followed either
        fs::remove_dir_all(r.join("StateSaves")).unwrap();
        std::os::unix::fs::symlink(out.path(), r.join("StateSaves")).unwrap();
        assert!(list(r, "RMCE01", "wii").unwrap().is_empty());
        // a hostile disc id cannot climb out
        assert!(list(r, "../..", "wii").unwrap().is_empty());
    }

    #[test]
    fn a_huge_save_dir_is_capped() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("Wii/title/00010000/524d4345/data");
        fs::create_dir_all(&d).unwrap();
        for i in 0..(MAX_WALK + 50) {
            fs::write(d.join(format!("f{i}")), b"x").unwrap();
        }
        let v = list(t.path(), "RMCE01", "wii").unwrap();
        assert!(v[0].files.unwrap() as usize <= MAX_WALK);
    }

    /// A ROM folder holding `Game.v1.nds`, plus an empty melonDS config dir.
    fn melon() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let (rom, cfg) = (t.path().join("nds/Game.v1.nds"), t.path().join("cfg"));
        put(t.path(), "nds/Game.v1.nds", 1);
        fs::create_dir(&cfg).unwrap();
        (t, rom, cfg)
    }

    #[test]
    fn melonds_lists_the_save_and_state_slots_next_to_the_rom() {
        let (t, rom, cfg) = melon();
        for (f, len) in [
            ("Game.v1.sav", 512),
            ("Game.v1.ml1", 10),
            ("Game.v1.ml8", 20),
            ("Game.v1.ml9", 1),
            ("Game.v1.mln", 1),
            ("Game.v1.ml1.sav", 1),
            ("Other.sav", 1),
        ] {
            put(t.path(), &format!("nds/{f}"), len);
        }
        let v = list_melonds(&rom, &cfg).unwrap();
        let mut got: Vec<_> = v
            .iter()
            .map(|i| (i.kind, i.path.as_str(), i.size))
            .collect();
        got.sort_by_key(|g| g.1);
        assert_eq!(
            got,
            [
                (Kind::State, "Game.v1.ml1", 10),
                (Kind::State, "Game.v1.ml8", 20),
                (Kind::GameSave, "Game.v1.sav", 512),
            ]
        );
        assert!(v.iter().all(|i| i.name == i.path && i.modified.is_some()));
    }

    #[test]
    fn melonds_nothing_yet_is_empty_and_no_rom_folder_is_none() {
        let (t, rom, cfg) = melon();
        assert_eq!(list_melonds(&rom, &cfg).unwrap(), vec![]);
        assert!(list_melonds(&t.path().join("gone/Game.nds"), &cfg).is_none());
    }

    #[test]
    fn melonds_honors_the_save_and_state_folders_from_its_config() {
        let (t, rom, cfg) = melon();
        let (sd, st) = (t.path().join("saves"), t.path().join("states"));
        put(t.path(), "nds/Game.v1.sav", 1);
        put(t.path(), "nds/Game.v1.ml2", 1);
        put(t.path(), "saves/Game.v1.sav", 7);
        put(t.path(), "states/Game.v1.ml3", 9);
        let toml = format!(
            "[Instance1]\nSaveFilePath = \"/elsewhere\"\n\n[Instance0]\nEnableCheats = false\n\
             SaveFilePath = \"{}/\"\nSavestatePath = '{}'\n\n[Instance0.Keyboard]\nA = 1\n",
            sd.display(),
            st.display()
        );
        fs::write(cfg.join("melonDS.toml"), toml).unwrap();
        let v = list_melonds(&rom, &cfg).unwrap();
        let mut got: Vec<_> = v.iter().map(|i| (i.path.as_str(), i.size)).collect();
        got.sort();
        assert_eq!(got, [("Game.v1.ml3", 9), ("Game.v1.sav", 7)]);
    }

    #[test]
    fn melonds_config_scan_ignores_what_it_cannot_trust() {
        let d = |t: &str| melon_dir(t, "SaveFilePath");
        assert_eq!(d("[Instance0]\nSaveFilePath = \"/s\""), Some("/s".into()));
        assert_eq!(d("[Instance0]\nSaveFilePath = \"\""), None);
        assert_eq!(d("[Instance0]\nSaveFilePath = \"rel/dir\""), None);
        assert_eq!(d("[Instance0]\nSaveFilePath = \"/a\\\"b\""), None);
        assert_eq!(d("SaveFilePath = \"/root-level\""), None);
        assert_eq!(d("[Instance0.X]\nSaveFilePath = \"/s\""), None);
    }

    #[test]
    fn melonds_skips_symlinks_and_folders() {
        let (t, rom, cfg) = melon();
        let out = tempfile::tempdir().unwrap();
        put(out.path(), "secret.bin", 3);
        let r = t.path().join("nds");
        std::os::unix::fs::symlink(out.path().join("secret.bin"), r.join("Game.v1.sav")).unwrap();
        fs::create_dir(r.join("Game.v1.ml1")).unwrap();
        put(t.path(), "nds/Game.v1.ml2", 4);
        let v = list_melonds(&rom, &cfg).unwrap();
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].path, "Game.v1.ml2");
    }
}
