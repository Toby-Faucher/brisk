//! Your notes in `data_dir/notes.json`: one per game, and tags you make (name, color, symbol, a note of
//! their own). Written whole to a 0600 temp file and renamed over the old one, read tolerantly (a missing
//! or corrupt file is just empty, and stays on disk until the next save replaces it). Private text: never
//! logged. Writes are per tag or per game, so two browsers editing different games never clash.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub const MAX_TAGS: usize = 64;
pub const MAX_GAMES: usize = 2000;
pub const TAG_ID_MAX: usize = 16;
pub const TAG_NAME_MAX: usize = 24;
pub const TAG_NOTE_MAX: usize = 500;
pub const GAME_NOTE_MAX: usize = 4000;
pub const GAME_TAGS_MAX: usize = 16;
pub const SYMBOL_MAX: usize = 16;
pub const COLORS: [&str; 9] = [
    "gray", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tag {
    pub name: String,
    pub color: String,
    /// A glyph name; the page owns the list and draws an unknown one as a dot.
    pub symbol: String,
    #[serde(default)]
    pub note: String,
    /// Shown as a card on Home.
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameNote {
    #[serde(default)]
    pub note: String,
    /// Tag ids, in the order given.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Notes {
    #[serde(default)]
    pub tags: BTreeMap<String, Tag>,
    #[serde(default)]
    /// Keyed by game id (`steam:620`).
    pub games: BTreeMap<String, GameNote>,
}

#[derive(Debug, PartialEq)]
pub enum NotesError {
    /// 400, with the reason.
    Bad(&'static str),
    /// 409: MAX_TAGS or MAX_GAMES reached.
    Full,
    /// 500: the file could not be written; nothing changed.
    Io,
}

pub struct NotesStore {
    path: PathBuf,
    max_tags: usize,
    max_games: usize,
    inner: Mutex<Notes>,
}

fn charset(s: &str, max: usize, ok: impl Fn(u8) -> bool) -> bool {
    (1..=max).contains(&s.len()) && s.bytes().all(ok)
}

pub fn valid_tag_id(id: &str) -> bool {
    crate::fsutil::slug_ok(id, TAG_ID_MAX)
}

/// At most `max` chars (not bytes), trailing whitespace not counted. The text itself is kept as sent.
fn check_note(s: &str, max: usize) -> Result<(), NotesError> {
    if s.trim_end().chars().count() > max {
        return Err(NotesError::Bad("note is too long"));
    }
    Ok(())
}

/// A game entry worth keeping: some text or a tag.
fn keep(g: &GameNote) -> bool {
    !g.note.trim().is_empty() || !g.tags.is_empty()
}

impl NotesStore {
    pub fn load(path: PathBuf) -> NotesStore {
        Self::with_limits(path, MAX_TAGS, MAX_GAMES)
    }

    /// `load` with other caps (tests).
    pub fn with_limits(path: PathBuf, max_tags: usize, max_games: usize) -> NotesStore {
        let mut n: Notes = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let tags = &n.tags;
        for g in n.games.values_mut() {
            g.tags.retain(|id| tags.contains_key(id));
        }
        n.games.retain(|_, g| keep(g));
        NotesStore {
            path,
            max_tags,
            max_games,
            inner: Mutex::new(n),
        }
    }

    pub fn all(&self) -> Notes {
        self.inner.lock().unwrap().clone()
    }

    /// Creates or replaces tag `id`; returns it as stored (name trimmed).
    pub fn put_tag(&self, id: &str, mut t: Tag) -> Result<Tag, NotesError> {
        // `auto-` ids belong to the page's smart tags.
        if !valid_tag_id(id) || id.starts_with("auto-") {
            return Err(NotesError::Bad("tag id must be 1-16 of a-z 0-9 -"));
        }
        t.name = t.name.trim().to_string();
        if !(1..=TAG_NAME_MAX).contains(&t.name.chars().count()) {
            return Err(NotesError::Bad("tag name must be 1-24 characters"));
        }
        if !COLORS.contains(&t.color.as_str()) {
            return Err(NotesError::Bad("unknown color"));
        }
        if !charset(&t.symbol, SYMBOL_MAX, |b| {
            b.is_ascii_lowercase() || b == b'-'
        }) {
            return Err(NotesError::Bad("symbol must be 1-16 of a-z -"));
        }
        check_note(&t.note, TAG_NOTE_MAX)?;
        self.change(|n| {
            if !n.tags.contains_key(id) && n.tags.len() >= self.max_tags {
                return Err(NotesError::Full);
            }
            n.tags.insert(id.to_string(), t.clone());
            Ok(())
        })?;
        Ok(t)
    }

    /// Removes the tag and takes it off every game (a game left empty goes too). Unknown id: no-op.
    pub fn delete_tag(&self, id: &str) -> Result<(), NotesError> {
        if !self.inner.lock().unwrap().tags.contains_key(id) {
            return Ok(());
        }
        self.change(|n| {
            n.tags.remove(id);
            for g in n.games.values_mut() {
                g.tags.retain(|t| t != id);
            }
            n.games.retain(|_, g| keep(g));
            Ok(())
        })
    }

    /// Stores one game's note and tags (unknown and repeated tag ids dropped); a blank note with no
    /// tags removes the entry. Returns what was stored.
    pub fn put_game(&self, id: &str, g: GameNote) -> Result<GameNote, NotesError> {
        check_note(&g.note, GAME_NOTE_MAX)?;
        let mut out = GameNote {
            note: g.note,
            tags: vec![],
        };
        self.change(|n| {
            for id in &g.tags {
                if n.tags.contains_key(id) && !out.tags.contains(id) {
                    out.tags.push(id.clone());
                }
            }
            if out.tags.len() > GAME_TAGS_MAX {
                return Err(NotesError::Bad("a game can have at most 16 tags"));
            }
            if !keep(&out) {
                n.games.remove(id);
            } else if !n.games.contains_key(id) && n.games.len() >= self.max_games {
                return Err(NotesError::Full);
            } else {
                n.games.insert(id.into(), out.clone());
            }
            Ok(())
        })?;
        Ok(out)
    }

    /// Moves entries from old game keys to new ones (`{old: new}`), keeping the file's `.pre-ids`
    /// copy first. A new key that is already there wins and the old entry stays. Returns how many moved.
    pub fn rekey(&self, map: &BTreeMap<String, String>) -> Result<usize, NotesError> {
        let hit = |n: &Notes| map.keys().any(|k| n.games.contains_key(k));
        if !hit(&self.inner.lock().unwrap()) {
            return Ok(0);
        }
        crate::migrate::backup_once(&self.path).map_err(|_| NotesError::Io)?;
        let mut moved = 0;
        self.change(|n| {
            for (old, new) in map {
                if !n.games.contains_key(new)
                    && let Some(g) = n.games.remove(old)
                {
                    n.games.insert(new.clone(), g);
                    moved += 1;
                }
            }
            Ok(())
        })?;
        Ok(moved)
    }

    /// Applies `f` to a copy, writes it, and only then keeps it: a failed write changes nothing.
    fn change(
        &self,
        f: impl FnOnce(&mut Notes) -> Result<(), NotesError>,
    ) -> Result<(), NotesError> {
        let mut m = self.inner.lock().unwrap();
        let mut next = m.clone();
        f(&mut next)?;
        if next != *m {
            self.save(&next).map_err(|e| {
                // the path and the error only, never the text
                eprintln!("warning: {}: {e}", self.path.display());
                NotesError::Io
            })?;
            *m = next;
        }
        Ok(())
    }

    fn save(&self, n: &Notes) -> std::io::Result<()> {
        crate::fsutil::write_atomic(&self.path, &serde_json::to_vec(n)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str) -> Tag {
        Tag {
            name: name.into(),
            color: "blue".into(),
            symbol: "star".into(),
            note: String::new(),
            pinned: false,
        }
    }

    #[test]
    fn put_tag_roundtrip_and_persists_across_load() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notes.json");
        let s = NotesStore::load(p.clone());
        assert_eq!(s.put_tag("a", tag(" A ")).unwrap().name, "A");
        s.put_game(
            "steam:1",
            GameNote {
                note: "x".into(),
                tags: vec!["a".into()],
            },
        )
        .unwrap();
        let back = NotesStore::load(p).all();
        assert_eq!(back.tags["a"], tag("A"));
        assert_eq!(back.games["steam:1"].tags, vec!["a".to_string()]);
    }

    #[test]
    fn auto_prefix_is_reserved() {
        let d = tempfile::tempdir().unwrap();
        let s = NotesStore::load(d.path().join("notes.json"));
        assert!(s.put_tag("auto-coop", tag("A")).is_err());
        assert!(s.put_tag("autos", tag("A")).is_ok());
        assert!(s.put_tag("a-auto", tag("A")).is_ok());
    }

    #[test]
    fn game_note_unicode_counts_chars_not_bytes() {
        let d = tempfile::tempdir().unwrap();
        let s = NotesStore::load(d.path().join("notes.json"));
        let g = |n| GameNote {
            note: "é".repeat(n),
            tags: vec![],
        };
        assert!(s.put_game("steam:1", g(GAME_NOTE_MAX)).is_ok());
        assert_eq!(
            s.put_game("steam:1", g(GAME_NOTE_MAX + 1)),
            Err(NotesError::Bad("note is too long"))
        );
    }

    #[test]
    fn corrupt_file_loads_empty_and_next_save_works() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notes.json");
        std::fs::write(&p, b"\xff\xfe garbage").unwrap();
        let s = NotesStore::load(p.clone());
        assert_eq!(s.all(), Notes::default());
        s.put_tag("a", tag("A")).unwrap();
        assert_eq!(NotesStore::load(p).all().tags.len(), 1);
    }

    #[test]
    fn load_drops_game_tags_that_are_not_tags() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notes.json");
        std::fs::write(
            &p,
            r#"{"tags":{"a":{"name":"A","color":"red","symbol":"star","note":""}},"games":{"steam:1":{"note":"","tags":["a","gone"]},"2":{"note":" ","tags":["gone"]}}}"#,
        )
        .unwrap();
        let games = NotesStore::load(p).all().games;
        assert_eq!(games["steam:1"].tags, vec!["a".to_string()]);
        assert!(!games.contains_key("2"), "left empty, so dropped");
    }

    #[test]
    fn a_failed_write_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        // the "file" is a directory, so the rename fails
        let p = d.path().join("notes.json");
        std::fs::create_dir(&p).unwrap();
        std::fs::write(p.join("x"), b"").unwrap();
        let s = NotesStore::load(p);
        assert_eq!(s.put_tag("a", tag("A")), Err(NotesError::Io));
        assert!(s.all().tags.is_empty());
    }
}
