//! Stage 1 data migration: `emulators.json` becomes `library.json`, and the files keyed by the old
//! numeric game ids (notes, prefs, sessions, last-played) are rekeyed to the new string ids.
//!
//! The conversions are pure functions (text in, text out) with fixtures in
//! `tests/fixtures/stage1/`. [`run`] is only the file handling, at startup before the server
//! accepts requests: it backs a file up once to `<file>.pre-ids` before the first rewrite, writes a
//! temp file and renames it, leaves a file it cannot parse alone (with a warning), and does nothing
//! on a second run. It never deletes `emulators.json`.
//!
//! Key rules: a new-form id stays; a number below 0xE0000000 becomes `steam:<n>`; a number from
//! there up becomes the library's id for that old hash, or stays as it is (and is logged) when no
//! library game has it. Nothing is dropped.

use crate::emu::{self, Record, legacy_hash};
use crate::fsutil::write_atomic;
use crate::game::{GameId, LEGACY_EMU_BASE};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Old hash id -> new id text.
pub type LegacyMap = HashMap<u32, String>;

const BACKUP_SUFFIX: &str = ".pre-ids";

/// Lowercase ASCII letters and digits, anything else a single `-`; at most 48 chars. A name with
/// no ASCII letters at all (a Japanese title) falls back to `g` plus its old hash, so it is still
/// unique and stable.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.truncate(48);
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        format!("g{:08x}", legacy_hash(name))
    } else {
        out.into()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OldEntry {
    name: String,
    emulator: String,
    file: std::path::PathBuf,
}

/// The old hand-written list as library records. Ids are `emu:<emulator>:<slug of the name>`, a
/// second game with the same slug gets `~2`, then `~3`. An entry whose emulator name cannot be an
/// id is skipped with a warning. Err when the text is not a list of entries.
pub fn library_from_emulators(old: &str) -> Result<Vec<Record>, String> {
    let entries: Vec<OldEntry> = serde_json::from_str(old).map_err(|e| e.to_string())?;
    let mut used: HashSet<GameId> = HashSet::new();
    let mut out = vec![];
    for e in entries {
        let base = slug(&e.name);
        // An emulator name that cannot be part of an id would never find a free one.
        let id = GameId::emu(&e.emulator, &base).and_then(|_| {
            (1..)
                .map(|n| match n {
                    1 => base.clone(),
                    _ => format!("{base}~{n}"),
                })
                .filter_map(|s| GameId::emu(&e.emulator, &s))
                .find(|id| !used.contains(id))
        });
        let Some(id) = id else {
            eprintln!(
                "warning: {}: skipping {:?}, bad emulator name",
                emu::OLD_FILE,
                e.name
            );
            continue;
        };
        used.insert(id.clone());
        out.push(Record {
            id,
            emulator: e.emulator,
            system: None,
            file: e.file,
            legacy_id: Some(legacy_hash(&e.name)),
            name: e.name,
            added_at: None,
            source: "manual".into(),
            aliases: vec![],
        });
    }
    Ok(out)
}

/// Old numeric id to new id for each record.
fn legacy_map(recs: &[Record]) -> LegacyMap {
    recs.iter()
        .map(|r| (r.legacy(), r.id.to_string()))
        .collect()
}

enum Key {
    /// Already a new id (or not an id at all): leave it.
    Keep,
    To(String),
    /// A number that no library game has.
    Unmapped,
}

fn map_key(key: &str, legacy: &LegacyMap) -> Key {
    if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) {
        return match key.parse::<u32>() {
            Ok(n) if n < LEGACY_EMU_BASE => Key::To(GameId::steam(n).to_string()),
            Ok(n) => legacy.get(&n).cloned().map_or(Key::Unmapped, Key::To),
            Err(_) => Key::Unmapped,
        };
    }
    Key::Keep
}

/// What a conversion made: `text` is None when nothing changed; `unmapped` lists keys left alone.
#[derive(Debug, Default, PartialEq)]
pub struct Out {
    pub text: Option<String>,
    pub unmapped: Vec<String>,
}

/// Renames the game keys of `m` (all but `skip`). A key whose new name is already taken is left
/// as it is and reported, so no entry is overwritten or lost.
fn rekey(m: &mut Map<String, Value>, skip: &[&str], legacy: &LegacyMap) -> (bool, Vec<String>) {
    let mut changed = false;
    let mut unmapped = vec![];
    let keys: Vec<String> = m
        .keys()
        .filter(|k| !skip.contains(&k.as_str()))
        .cloned()
        .collect();
    for k in keys {
        match map_key(&k, legacy) {
            Key::Keep => {}
            Key::Unmapped => unmapped.push(k),
            Key::To(new) if m.contains_key(&new) => unmapped.push(k),
            Key::To(new) => {
                let v = m.remove(&k).unwrap_or(Value::Null);
                m.insert(new, v);
                changed = true;
            }
        }
    }
    (changed, unmapped)
}

fn object(text: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(text).map_err(|e| e.to_string())? {
        Value::Object(m) => Ok(m),
        _ => Err("not a JSON object".into()),
    }
}

fn done(v: Value, changed: bool, unmapped: Vec<String>) -> Result<Out, String> {
    Ok(Out {
        text: changed.then(|| v.to_string()),
        unmapped,
    })
}

/// `notes.json`: the keys of `games`.
pub fn notes(old: &str, legacy: &LegacyMap) -> Result<Out, String> {
    let mut root = object(old)?;
    let (changed, unmapped) = match root.get_mut("games") {
        Some(Value::Object(g)) => rekey(g, &[], legacy),
        Some(_) => return Err("games is not an object".into()),
        None => (false, vec![]),
    };
    done(Value::Object(root), changed, unmapped)
}

/// `prefs.json`: every key but `layout`.
pub fn prefs(old: &str, legacy: &LegacyMap) -> Result<Out, String> {
    let mut root = object(old)?;
    let (changed, unmapped) = rekey(&mut root, &["layout"], legacy);
    done(Value::Object(root), changed, unmapped)
}

/// `emu-played.json`: every key.
pub fn played(old: &str, legacy: &LegacyMap) -> Result<Out, String> {
    let mut root = object(old)?;
    let (changed, unmapped) = rekey(&mut root, &[], legacy);
    done(Value::Object(root), changed, unmapped)
}

/// `sessions.jsonl`: a row with an `appid` and no `gameId` gets one; `appid` stays for old readers.
/// A line that is not a JSON object is kept as it is. Desktop rows (`appid` null) have no game.
pub fn sessions(old: &str, legacy: &LegacyMap) -> Result<Out, String> {
    let mut changed = false;
    let mut unmapped = vec![];
    let mut lines: Vec<String> = vec![];
    for line in old.lines() {
        let Ok(Value::Object(mut row)) = serde_json::from_str::<Value>(line) else {
            lines.push(line.into());
            continue;
        };
        let Some(n) = row
            .get("appid")
            .and_then(Value::as_u64)
            .filter(|_| !row.contains_key("gameId"))
        else {
            lines.push(line.into());
            continue;
        };
        match map_key(&n.to_string(), legacy) {
            Key::To(id) => {
                row.insert("gameId".into(), id.into());
                changed = true;
                lines.push(Value::Object(row).to_string());
            }
            _ => {
                unmapped.push(n.to_string());
                lines.push(line.into());
            }
        }
    }
    unmapped.sort();
    unmapped.dedup();
    let text = changed.then(|| lines.join("\n") + if old.ends_with('\n') { "\n" } else { "" });
    Ok(Out { text, unmapped })
}

/// Copies `path` to `<path>.pre-ids` unless that copy exists already (made once). A missing file is fine.
pub fn backup_once(path: &Path) -> std::io::Result<()> {
    let mut b = path.as_os_str().to_owned();
    b.push(BACKUP_SUFFIX);
    let b = std::path::PathBuf::from(b);
    match std::fs::read(path) {
        Ok(bytes) if !b.exists() => write_atomic(&b, &bytes),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Applies `convert` to `data_dir/name`: missing = skip, unparseable = warn and leave, unchanged
/// = nothing written. Otherwise the original is copied to `<name>.pre-ids` unless that exists
/// (made once), then the new text is written to a temp file and renamed over it.
fn apply(data_dir: &Path, name: &str, convert: impl Fn(&str) -> Result<Out, String>) {
    let path = data_dir.join(name);
    let Ok(old) = std::fs::read_to_string(&path) else {
        return;
    };
    let out = match convert(&old) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("warning: {name}: left alone, not migrated: {e}");
            return;
        }
    };
    if !out.unmapped.is_empty() {
        eprintln!(
            "warning: {name}: {} keys kept as they are (no game has that id): {}",
            out.unmapped.len(),
            out.unmapped.join(", ")
        );
    }
    let Some(new) = out.text else {
        return;
    };
    let go = || -> std::io::Result<()> {
        backup_once(&path)?;
        write_atomic(&path, new.as_bytes())
    };
    match go() {
        Ok(()) => eprintln!(
            "info: {name}: keys moved to the new game ids (backup: {name}{BACKUP_SUFFIX})"
        ),
        Err(e) => eprintln!("warning: {name}: not migrated: {e}"),
    }
}

/// The startup migration. Safe to run on every start.
pub fn run(data_dir: &Path) {
    let lib = data_dir.join(emu::LIBRARY_FILE);
    if !lib.exists()
        && let Ok(text) = std::fs::read_to_string(data_dir.join(emu::OLD_FILE))
    {
        match library_from_emulators(&text) {
            Ok(recs) => match write_atomic(&lib, &emu::library_json(&recs)) {
                Ok(()) => eprintln!(
                    "info: {} -> {} ({} games)",
                    emu::OLD_FILE,
                    emu::LIBRARY_FILE,
                    recs.len()
                ),
                Err(e) => eprintln!("warning: {}: {e}", lib.display()),
            },
            Err(e) => eprintln!("warning: {} left alone, not migrated: {e}", emu::OLD_FILE),
        }
    }
    let legacy = legacy_map(&emu::load(data_dir));
    apply(data_dir, "notes.json", |t| notes(t, &legacy));
    apply(data_dir, "prefs.json", |t| prefs(t, &legacy));
    apply(data_dir, "sessions.jsonl", |t| sessions(t, &legacy));
    apply(data_dir, "emu-played.json", |t| played(t, &legacy));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lm(pairs: &[(u32, &str)]) -> LegacyMap {
        pairs.iter().map(|(n, s)| (*n, s.to_string())).collect()
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Mario Kart Wii"), "mario-kart-wii");
        assert_eq!(
            slug("  Super Smash Bros. Melee!! "),
            "super-smash-bros-melee"
        );
        assert_eq!(
            slug("The Legend of Zelda: Twilight Princess"),
            "the-legend-of-zelda-twilight-princess"
        );
        assert_eq!(slug(&"a".repeat(80)).len(), 48);
        // nothing ASCII to keep: a stable fallback from the old hash
        assert_eq!(slug("ゼルダ"), format!("g{:08x}", legacy_hash("ゼルダ")));
        assert_eq!(slug("ゼルダ"), slug("ゼルダ"));
        // always a valid id part
        for n in ["", "-", "é", "a b", "x".repeat(100).as_str()] {
            assert!(GameId::emu("dolphin", &slug(n)).is_some(), "{n:?}");
        }
    }

    #[test]
    fn library_from_old_list() {
        let old = r#"[
            {"name":"Mario Kart Wii","emulator":"dolphin","file":"/g/a.iso"},
            {"name":"Mario Kart: Wii!","emulator":"dolphin","file":"/g/b.iso"},
            {"name":"Mario Kart Wii?","emulator":"dolphin","file":"/g/c.iso"},
            {"name":"Same name elsewhere","emulator":"other","file":"/g/d.iso"},
            {"name":"Mario Kart Wii","emulator":"other","file":"/g/e.iso"},
            {"name":"Bad","emulator":"Not An Id","file":"/g/f.iso"}]"#;
        let l = library_from_emulators(old).unwrap();
        let ids: Vec<_> = l.iter().map(|r| r.id.to_string()).collect();
        assert_eq!(
            ids,
            [
                "emu:dolphin:mario-kart-wii",
                "emu:dolphin:mario-kart-wii~2",
                "emu:dolphin:mario-kart-wii~3",
                "emu:other:same-name-elsewhere",
                "emu:other:mario-kart-wii",
            ]
        );
        assert_eq!(l[0].legacy(), legacy_hash("Mario Kart Wii"));
        assert_eq!(l[1].legacy(), legacy_hash("Mario Kart: Wii!"));
        assert!(
            l.iter()
                .all(|r| r.source == "manual" && r.aliases.is_empty())
        );
        // order, names and files are kept
        assert_eq!(l[2].file, Path::new("/g/c.iso"));
        assert!(library_from_emulators("{not json").is_err());
        assert!(
            library_from_emulators(r#"[{"name":"A","emulator":"dolphin","file":"/a","extra":1}]"#)
                .is_err()
        );
        assert_eq!(library_from_emulators("[]").unwrap(), vec![]);
    }

    #[test]
    fn key_rules() {
        let m = lm(&[(0xE000_0001, "emu:dolphin:a")]);
        let n = notes(
            r#"{"tags":{},"games":{"620":{"note":"x"},"steam:5":{"note":"y"},"3758096385":{"note":"z"},"3758096999":{"note":"u"},"emu:dolphin:q":{"note":"w"},"weird":{"note":"v"},"0":{"note":"d"}}}"#,
            &m,
        )
        .unwrap();
        let v: Value = serde_json::from_str(&n.text.unwrap()).unwrap();
        let keys: Vec<&str> = v["games"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "3758096999",
                "emu:dolphin:a",
                "emu:dolphin:q",
                "steam:0",
                "steam:5",
                "steam:620",
                "weird"
            ]
        );
        assert_eq!(n.unmapped, ["3758096999"]);
    }

    #[test]
    fn a_new_key_already_there_wins_and_the_old_one_stays() {
        let o = prefs(r#"{"620":{"a":1},"steam:620":{"a":2}}"#, &lm(&[])).unwrap();
        assert_eq!(o.text, None);
        assert_eq!(o.unmapped, ["620"]);
    }

    #[test]
    fn prefs_keep_the_layout_and_unchanged_means_no_text() {
        let o = prefs(r#"{"layout":{"v":1,"homeOrder":["a"]}}"#, &lm(&[])).unwrap();
        assert_eq!(o, Out::default());
        let o = prefs(r#"{"620":{"a":1},"layout":{"v":1}}"#, &lm(&[])).unwrap();
        let v: Value = serde_json::from_str(&o.text.unwrap()).unwrap();
        assert_eq!(v, json!({"steam:620":{"a":1},"layout":{"v":1}}));
        assert!(prefs("[1]", &lm(&[])).is_err());
        assert!(prefs("", &lm(&[])).is_err());
        assert!(notes(r#"{"games":[]}"#, &lm(&[])).is_err());
        assert_eq!(notes(r#"{"tags":{}}"#, &lm(&[])).unwrap(), Out::default());
    }

    #[test]
    fn sessions_rows() {
        let m = lm(&[(0xE000_0001, "emu:dolphin:a")]);
        let old = concat!(
            "{\"appid\":620,\"name\":\"P\"}\n",
            "{\"appid\":null,\"name\":\"Desktop\"}\n",
            "{\"appid\":3758096385,\"name\":\"A\"}\n",
            "{\"appid\":5,\"name\":\"has\",\"gameId\":\"steam:5\"}\n",
            "{\"appid\":3758096999,\"name\":\"gone\"}\n",
            "not json at all\n",
            "[1,2]\n",
        );
        let o = sessions(old, &m).unwrap();
        let t = o.text.unwrap();
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 7);
        let id = |i: usize| {
            serde_json::from_str::<Value>(lines[i])
                .ok()
                .and_then(|v| v["gameId"].as_str().map(String::from))
        };
        assert_eq!(id(0).as_deref(), Some("steam:620"));
        assert_eq!(id(1), None);
        assert_eq!(id(2).as_deref(), Some("emu:dolphin:a"));
        assert_eq!(id(3).as_deref(), Some("steam:5"));
        assert_eq!(id(4), None);
        // lines it does not change are byte for byte
        assert_eq!(lines[1], "{\"appid\":null,\"name\":\"Desktop\"}");
        assert_eq!(lines[5], "not json at all");
        assert!(t.ends_with('\n'));
        assert_eq!(o.unmapped, ["3758096999"]);
        // a second pass changes nothing
        assert_eq!(sessions(&t, &m).unwrap().text, None);
        // no trailing newline stays none
        assert!(
            !sessions("{\"appid\":1}", &m)
                .unwrap()
                .text
                .unwrap()
                .ends_with('\n')
        );
    }
}
