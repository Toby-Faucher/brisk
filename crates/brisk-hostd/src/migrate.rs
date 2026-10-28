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

use crate::emu::{self, Record, legacy_hash, write_atomic};
use crate::game::{GameId, LEGACY_EMU_BASE};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Old hash id -> new id text.
pub type LegacyMap = HashMap<u32, String>;

pub const BACKUP_SUFFIX: &str = ".pre-ids";

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
        let id = (1..)
            .map(|n| {
                if n == 1 {
                    base.clone()
                } else {
                    format!("{base}~{n}")
                }
            })
            .filter_map(|s| GameId::emu(&e.emulator, &s))
            .find(|id| !used.contains(id));
        let Some(id) = id else {
            eprintln!(
                "warn: {}: skipping {:?}, bad emulator name",
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

pub fn legacy_map(recs: &[Record]) -> LegacyMap {
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
            eprintln!("warn: {name}: left alone, not migrated: {e}");
            return;
        }
    };
    if !out.unmapped.is_empty() {
        eprintln!(
            "warn: {name}: {} keys kept as they are (no game has that id): {}",
            out.unmapped.len(),
            out.unmapped.join(", ")
        );
    }
    let Some(new) = out.text else {
        return;
    };
    let backup = data_dir.join(format!("{name}{BACKUP_SUFFIX}"));
    let go = || -> std::io::Result<()> {
        if !backup.exists() {
            write_atomic(&backup, old.as_bytes())?;
        }
        write_atomic(&path, new.as_bytes())
    };
    match go() {
        Ok(()) => eprintln!(
            "info: {name}: keys moved to the new game ids (backup: {name}{BACKUP_SUFFIX})"
        ),
        Err(e) => eprintln!("warn: {name}: not migrated: {e}"),
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
                Err(e) => eprintln!("warn: {}: {e}", lib.display()),
            },
            Err(e) => eprintln!("warn: {} left alone, not migrated: {e}", emu::OLD_FILE),
        }
    }
    let legacy = legacy_map(&emu::load(data_dir));
    apply(data_dir, "notes.json", |t| notes(t, &legacy));
    apply(data_dir, "prefs.json", |t| prefs(t, &legacy));
    apply(data_dir, "sessions.jsonl", |t| sessions(t, &legacy));
    apply(data_dir, "emu-played.json", |t| played(t, &legacy));
}
