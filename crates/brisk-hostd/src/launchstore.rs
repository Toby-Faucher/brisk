//! `data_dir/launch.json`: launch option defaults, per-game overrides and the "allow custom commands"
//! switch. Written whole to a 0600 temp file and renamed over the old one; read tolerantly (missing =
//! empty defaults; oversized or corrupt = copied to `.corrupt` once, then empty defaults; bad entries dropped). Nothing read from it is trusted:
//! `command_for` validates again at every use, and an invalid result launches with no options.

use crate::game::GameId;
use crate::launch::{self, Command, LaunchOptions, LaunchOverride, Problem};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_FILE: u64 = 256 << 10;
pub(crate) const MAX_GAMES: usize = 500;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    /// Only changeable with the defaults.
    pub allow_custom_commands: bool,
    pub defaults: LaunchOptions,
    /// Keyed by game id (`steam:620`).
    pub games: BTreeMap<String, LaunchOverride>,
}

/// Reads the file; never fails. Entries that do not parse, or whose key is no game id, are dropped.
pub(crate) fn read_file(path: &Path) -> Data {
    let mut b = Vec::new();
    // A file we cannot use is copied aside first: the next save would overwrite it.
    let v: Value =
        match std::fs::File::open(path).and_then(|f| f.take(MAX_FILE + 1).read_to_end(&mut b)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
            Err(e) => {
                crate::fsutil::set_aside(path, &e);
                Value::Null
            }
            Ok(n) if n as u64 > MAX_FILE => {
                crate::fsutil::set_aside(path, &"file too large");
                Value::Null
            }
            Ok(_) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                crate::fsutil::set_aside(path, &e);
                Value::Null
            }),
        };
    let mut d = Data {
        allow_custom_commands: v["allowCustomCommands"].as_bool() == Some(true),
        defaults: serde_json::from_value(v["defaults"].clone()).unwrap_or_default(),
        games: BTreeMap::new(),
    };
    if let Some(m) = v["games"].as_object() {
        for (k, o) in m.iter().take(MAX_GAMES) {
            if let (Some(id), Ok(o)) = (GameId::parse(k), serde_json::from_value(o.clone())) {
                d.games.insert(id.to_string(), o);
            }
        }
    }
    d
}

impl Data {
    /// The options that apply to game `id`.
    pub fn effective(&self, id: &str) -> LaunchOptions {
        launch::effective(&self.defaults, self.games.get(id))
    }
}

/// What to run for `id` around `base`, and a note when saved options were unusable and ignored.
/// Never fails: invalid options (for example custom commands switched off later) mean no options.
pub(crate) fn command_for(d: &Data, id: &str, base: &[String]) -> (Command, Option<String>) {
    match launch::validate(&d.effective(id), d.allow_custom_commands) {
        Ok(v) => (launch::build(&v, base), None),
        Err(p) => (
            plain(base),
            Some(format!(
                "launch options for {id} ignored: {}",
                describe_all(&p)
            )),
        ),
    }
}

/// The command `base` as it is, with no wrappers.
pub fn plain(base: &[String]) -> Command {
    Command {
        program: base.first().cloned().unwrap_or_default(),
        args: base.iter().skip(1).cloned().collect(),
        env: vec![],
    }
}

/// A problem as a short sentence for the page.
pub fn describe(p: &Problem) -> String {
    match p {
        Problem::DuplicateWrapper(w) => format!("{w} is listed twice"),
        Problem::GamescopeArg(e) => format!("gamescope: {e}"),
        Problem::EnvName(n) => format!("environment variable {n:?} is not on the allowed list"),
        Problem::EnvValue(n) => format!(
            "the value of {n} is too long or has a line break or NUL (512 characters at most)"
        ),
        Problem::ExtraArgs(e) => e.clone(),
        Problem::CustomNotAllowed => {
            "custom commands are switched off; turn on Allow custom commands in Settings".into()
        }
        Problem::Custom(e) => format!("custom command: {e}"),
    }
}

/// All problems in one line.
fn describe_all(p: &[Problem]) -> String {
    p.iter().map(describe).collect::<Vec<_>>().join("; ")
}

#[derive(Debug)]
pub enum StoreError {
    Bad(Vec<String>),
    /// The thing changed since `rev`.
    Changed,
    Full,
    Io,
}

pub struct LaunchStore {
    path: PathBuf,
    inner: Mutex<Data>,
}

fn rev_of<T: Serialize>(t: &T) -> String {
    use std::hash::Hasher;
    // shortcut: DefaultHasher is only compared inside one hostd run, like controls.rs.
    let mut h = std::hash::DefaultHasher::new();
    h.write(&serde_json::to_vec(t).unwrap_or_default());
    format!("{:016x}", h.finish())
}

/// The revision of the defaults.
pub(crate) fn defaults_rev(d: &Data) -> String {
    rev_of(&(d.allow_custom_commands, &d.defaults))
}

/// The revision of one game's override.
pub(crate) fn game_rev(d: &Data, id: &str) -> String {
    rev_of(&d.games.get(id))
}

impl LaunchStore {
    /// The store at `path`; a missing file is empty, a corrupt one is copied to `<path>.corrupt` first.
    pub fn load(path: PathBuf) -> LaunchStore {
        LaunchStore {
            inner: Mutex::new(read_file(&path)),
            path,
        }
    }

    /// Everything stored.
    pub fn all(&self) -> Data {
        self.inner.lock().unwrap().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Data> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Replaces the defaults and the custom-commands switch. Must validate under the new switch.
    pub(crate) fn put_defaults(
        &self,
        rev: Option<&str>,
        allow: bool,
        o: LaunchOptions,
    ) -> Result<Data, StoreError> {
        let mut g = self.lock();
        if rev.is_some_and(|r| r != defaults_rev(&g)) {
            return Err(StoreError::Changed);
        }
        launch::validate(&o, allow).map_err(|p| bad(&p))?;
        let mut n = g.clone();
        n.allow_custom_commands = allow;
        n.defaults = o;
        self.commit(&mut g, n)
    }

    /// Replaces one game's override. The effective result must validate.
    pub(crate) fn put_game(
        &self,
        id: &str,
        rev: Option<&str>,
        o: LaunchOverride,
    ) -> Result<Data, StoreError> {
        let mut g = self.lock();
        if rev.is_some_and(|r| r != game_rev(&g, id)) {
            return Err(StoreError::Changed);
        }
        if !g.games.contains_key(id) && g.games.len() >= MAX_GAMES {
            return Err(StoreError::Full);
        }
        let eff = launch::effective(&g.defaults, Some(&o));
        launch::validate(&eff, g.allow_custom_commands).map_err(|p| bad(&p))?;
        let mut n = g.clone();
        n.games.insert(id.into(), o);
        self.commit(&mut g, n)
    }

    /// Removes the game's override.
    pub(crate) fn delete_game(&self, id: &str) -> Result<Data, StoreError> {
        let mut g = self.lock();
        let mut n = g.clone();
        n.games.remove(id);
        self.commit(&mut g, n)
    }

    fn commit(&self, cur: &mut Data, n: Data) -> Result<Data, StoreError> {
        if *cur != n {
            self.save(&n).map_err(|_| StoreError::Io)?;
            *cur = n;
        }
        Ok(cur.clone())
    }

    fn save(&self, d: &Data) -> std::io::Result<()> {
        crate::fsutil::write_atomic(&self.path, &serde_json::to_vec(d)?)
    }
}

fn bad(p: &[Problem]) -> StoreError {
    StoreError::Bad(p.iter().map(describe).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::Wrapper;
    use std::os::unix::fs::PermissionsExt;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }
    fn mango() -> LaunchOptions {
        LaunchOptions {
            wrappers: vec![Wrapper::MangoHud],
            ..Default::default()
        }
    }

    #[test]
    fn round_trip_is_0600_and_reloads() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launch.json");
        let st = LaunchStore::load(p.clone());
        st.put_defaults(None, false, mango()).unwrap();
        st.put_game(
            "steam:620",
            None,
            LaunchOverride {
                wrappers: Some(vec![Wrapper::GameMode]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(LaunchStore::load(p).all(), st.all());
        assert_eq!(
            st.all().effective("steam:620").wrappers,
            vec![Wrapper::GameMode]
        );
        assert_eq!(
            st.all().effective("steam:10").wrappers,
            vec![Wrapper::MangoHud]
        );
    }

    #[test]
    fn corrupt_huge_and_odd_files_read_as_empty_or_partial() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launch.json");
        assert_eq!(read_file(&p), Data::default());
        assert!(!d.path().join("launch.json.corrupt").exists());
        std::fs::write(&p, b"{not json").unwrap();
        assert_eq!(read_file(&p), Data::default());
        assert_eq!(
            std::fs::read(d.path().join("launch.json.corrupt")).unwrap(),
            b"{not json"
        );
        std::fs::write(&p, vec![b' '; (MAX_FILE + 1) as usize]).unwrap();
        assert_eq!(read_file(&p), Data::default());
        std::fs::write(
            &p,
            r#"{"allowCustomCommands":"yes","defaults":{"wrappers":["nope"]},
               "games":{"steam:1":{"wrappers":[{"gamescope":{"args":[]}}]},"steam:2":{"zzz":1},"junk":{},"steam:3":{}}}"#,
        )
        .unwrap();
        let r = read_file(&p);
        assert!(!r.allow_custom_commands);
        assert_eq!(r.defaults, LaunchOptions::default());
        assert_eq!(r.games.keys().collect::<Vec<_>>(), ["steam:1", "steam:3"]);
    }

    #[test]
    fn invalid_saved_options_are_ignored_with_a_note() {
        let mut d = Data {
            allow_custom_commands: true,
            defaults: LaunchOptions {
                wrappers: vec![Wrapper::MangoHud],
                custom: Some(s(&["mytool"])),
                ..Default::default()
            },
            ..Default::default()
        };
        let base = s(&["dolphin-emu", "-b"]);
        let (c, note) = command_for(&d, "steam:1", &base);
        assert_eq!((c.program.as_str(), note), ("mangohud", None));
        d.allow_custom_commands = false; // switched off later
        let (c, note) = command_for(&d, "steam:1", &base);
        assert_eq!((c.program.as_str(), c.args), ("dolphin-emu", s(&["-b"])));
        assert!(c.env.is_empty());
        assert!(note.unwrap().contains("Allow custom commands"));
    }

    #[test]
    fn put_checks_validity_rev_and_cap() {
        let d = tempfile::tempdir().unwrap();
        let st = LaunchStore::load(d.path().join("launch.json"));
        let custom = LaunchOptions {
            custom: Some(s(&["tool"])),
            ..Default::default()
        };
        assert!(matches!(
            st.put_defaults(None, false, custom.clone()),
            Err(StoreError::Bad(p)) if p[0].contains("switched off")
        ));
        st.put_defaults(None, true, custom).unwrap();
        // the override cannot keep the custom command once the switch is off
        let r1 = defaults_rev(&st.all());
        assert!(matches!(
            st.put_defaults(Some("stale"), false, LaunchOptions::default()),
            Err(StoreError::Changed)
        ));
        st.put_defaults(Some(&r1), true, LaunchOptions::default())
            .unwrap();
        let ov = LaunchOverride {
            env: Some([("LD_PRELOAD".to_string(), "x".to_string())].into()),
            ..Default::default()
        };
        assert!(matches!(
            st.put_game("steam:1", None, ov),
            Err(StoreError::Bad(_))
        ));
        assert!(st.all().games.is_empty());
        for i in 0..MAX_GAMES {
            st.put_game(&format!("steam:{i}"), None, LaunchOverride::default())
                .unwrap();
        }
        assert!(matches!(
            st.put_game("steam:99999", None, LaunchOverride::default()),
            Err(StoreError::Full)
        ));
        st.delete_game("steam:0").unwrap();
        st.delete_game("steam:0").unwrap();
    }
}
