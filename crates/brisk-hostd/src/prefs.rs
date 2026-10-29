//! Per-game quality in `data_dir/prefs.json` (`{"<game id>": Entry}`, ids like `steam:620`): written whole to a 0600 temp
//! file and renamed over the old one, read tolerantly (a missing or corrupt file is just empty).
//! The Home card order and the Machines page order ride along under
//! `"layout": {"v": 1, "homeOrder": [...], "machinesOrder": [...], "machineColors": {id: "#rrggbb"}}`
//! (each key absent when empty).

use crate::sessions::now_ms;
use crate::stream::{Codec, Preset};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub const MAX_ENTRIES: usize = 500;
/// Home cards are a handful; the caps only bound what a client can make hostd store.
const HOME_MAX: usize = 32;
const HOME_ID_MAX: usize = 32;
const MACHINES_MAX: usize = 64;
const COLORS_MAX: usize = 64;
const LAYOUT_KEY: &str = "layout";
const LAYOUT_V: u64 = 1;

/// Keeps the ids that are 1..=HOME_ID_MAX of `a-z 0-9 -`, first occurrence only, at most HOME_MAX.
fn clean_home_order(ids: Vec<String>) -> Vec<String> {
    clean_ids(ids, HOME_MAX)
}

/// Same rules as `clean_home_order`, at most MACHINES_MAX ids.
fn clean_machines_order(ids: Vec<String>) -> Vec<String> {
    clean_ids(ids, MACHINES_MAX)
}

fn valid_id(s: &str) -> bool {
    crate::fsutil::slug_ok(s, HOME_ID_MAX)
}

/// `#rgb` or `#rrggbb` in any case -> lowercase `#rrggbb`; anything else is None.
fn normalize_color(c: &str) -> Option<String> {
    let h = c.strip_prefix('#')?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match h.len() {
        6 => Some(c.to_ascii_lowercase()),
        3 => Some(format!(
            "#{}",
            h.chars()
                .flat_map(|d| [d, d])
                .collect::<String>()
                .to_ascii_lowercase()
        )),
        _ => None,
    }
}

fn clean_ids(ids: Vec<String>, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if out.len() == max {
            break;
        }
        if valid_id(&id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

#[derive(Default)]
struct Inner {
    map: BTreeMap<String, Entry>,
    home: Vec<String>,
    machines: Vec<String>,
    colors: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Stream quality saved for one game.
pub struct Entry {
    pub preset: Preset,
    pub codec: Codec,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate_kbps: Option<u32>,
    /// Unix milliseconds of the last write; the oldest go first when the file is full.
    pub updated: u64,
}

/// (homeOrder, machinesOrder, machineColors)
pub type Layout = (Vec<String>, Vec<String>, BTreeMap<String, String>);

/// Per-game stream quality and the layout prefs, kept in prefs.json.
pub struct Prefs {
    path: PathBuf,
    inner: Mutex<Inner>,
}

impl Prefs {
    /// The prefs at `path`; a missing or corrupt file is empty.
    pub fn load(path: PathBuf) -> Prefs {
        let mut raw: BTreeMap<String, serde_json::Value> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // another version (or garbage) reads as no saved order
        let layout = raw
            .remove(LAYOUT_KEY)
            .filter(|l| l["v"].as_u64() == Some(LAYOUT_V));
        let list = |key: &str, clean: fn(Vec<String>) -> Vec<String>| {
            layout
                .as_ref()
                .and_then(|l| serde_json::from_value::<Vec<String>>(l[key].clone()).ok())
                .map(clean)
                .unwrap_or_default()
        };
        let home = list("homeOrder", clean_home_order);
        let machines = list("machinesOrder", clean_machines_order);
        // invalid entries drop silently, the cap keeps the first ids in key order
        let colors = layout
            .as_ref()
            .and_then(|l| {
                serde_json::from_value::<BTreeMap<String, serde_json::Value>>(
                    l["machineColors"].clone(),
                )
                .ok()
            })
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(k, v)| valid_id(&k).then_some((k, normalize_color(v.as_str()?)?)))
            .take(COLORS_MAX)
            .collect();
        // Keys are kept as they are, even an old number the migration could not map (never dropped).
        let mut map: BTreeMap<String, Entry> = raw
            .into_iter()
            .filter(|(k, _)| {
                crate::game::GameId::parse(k).is_some() || k.bytes().all(|b| b.is_ascii_digit())
            })
            .filter_map(|(k, v)| Some((k, serde_json::from_value(v).ok()?)))
            .collect();
        evict(&mut map);
        Prefs {
            path,
            inner: Mutex::new(Inner {
                map,
                home,
                machines,
                colors,
            }),
        }
    }

    /// Every game's saved quality.
    pub fn all(&self) -> BTreeMap<String, Entry> {
        self.inner.lock().unwrap().map.clone()
    }

    /// The saved Home card order.
    pub(crate) fn home_order(&self) -> Vec<String> {
        self.inner.lock().unwrap().home.clone()
    }

    /// The saved Machines page order.
    pub(crate) fn machines_order(&self) -> Vec<String> {
        self.inner.lock().unwrap().machines.clone()
    }

    /// The saved machine colours.
    pub(crate) fn machine_colors(&self) -> BTreeMap<String, String> {
        self.inner.lock().unwrap().colors.clone()
    }

    /// Cleans, stores and writes the lists that are `Some` (the other keeps its value); an empty
    /// list clears that order. `colors` is merged into the stored map (None removes an id). A bad
    /// id or colour, or more than COLORS_MAX entries after the merge, is an Err and stores nothing.
    /// Returns (home, machines, colors) as stored.
    pub(crate) fn set_layout(
        &self,
        home: Option<Vec<String>>,
        machines: Option<Vec<String>>,
        colors: Option<BTreeMap<String, Option<String>>>,
    ) -> Result<Layout, &'static str> {
        let mut m = self.inner.lock().unwrap();
        let mut merged = m.colors.clone();
        for (id, c) in colors.unwrap_or_default() {
            if !valid_id(&id) {
                return Err("bad machine id in machineColors");
            }
            match c {
                None => merged.remove(&id),
                Some(c) => {
                    merged.insert(id, normalize_color(&c).ok_or("bad colour, want #rrggbb")?)
                }
            };
        }
        if merged.len() > COLORS_MAX {
            return Err("too many machineColors");
        }
        m.colors = merged;
        if let Some(h) = home {
            m.home = clean_home_order(h);
        }
        if let Some(x) = machines {
            m.machines = clean_machines_order(x);
        }
        self.save(&m);
        Ok((m.home.clone(), m.machines.clone(), m.colors.clone()))
    }

    /// Stores `e` with its `updated` as given and writes the file.
    pub fn put_entry(&self, id: &str, e: Entry) {
        let mut m = self.inner.lock().unwrap();
        m.map.insert(id.into(), e);
        evict(&mut m.map);
        self.save(&m);
    }

    /// Like `put_entry` with `updated` = now, but always newer than every entry already there.
    pub fn put(&self, id: &str, mut e: Entry) -> Entry {
        let mut m = self.inner.lock().unwrap();
        let newest = m.map.values().map(|e| e.updated).max().unwrap_or(0);
        e.updated = now_ms().max(newest + 1);
        m.map.insert(id.into(), e.clone());
        evict(&mut m.map);
        self.save(&m);
        e
    }

    /// Moves entries from old game keys to new ones (`{old: new}`), keeping the file's `.pre-ids`
    /// copy first. A new key that is already there wins and the old entry stays. Returns how many moved.
    pub fn rekey(&self, map: &BTreeMap<String, String>) -> usize {
        let mut m = self.inner.lock().unwrap();
        if !map.keys().any(|k| m.map.contains_key(k)) {
            return 0;
        }
        if let Err(e) = crate::migrate::backup_once(&self.path) {
            tracing::warn!("{}: no backup, not rekeyed: {e}", self.path.display());
            return 0;
        }
        let mut moved = 0;
        for (old, new) in map {
            if !m.map.contains_key(new)
                && let Some(e) = m.map.remove(old)
            {
                m.map.insert(new.clone(), e);
                moved += 1;
            }
        }
        self.save(&m);
        moved
    }

    /// Forgets the game's saved quality.
    pub fn delete(&self, id: &str) {
        let mut m = self.inner.lock().unwrap();
        if m.map.remove(id).is_some() {
            self.save(&m);
        }
    }

    /// A failed write is a warning: the change then lives in memory until hostd exits.
    fn save(&self, m: &Inner) {
        let write = || -> std::io::Result<()> {
            let mut doc = serde_json::to_value(&m.map)?;
            if !m.home.is_empty() || !m.machines.is_empty() || !m.colors.is_empty() {
                let mut l = serde_json::json!({ "v": LAYOUT_V });
                if !m.home.is_empty() {
                    l["homeOrder"] = m.home.clone().into();
                }
                if !m.machines.is_empty() {
                    l["machinesOrder"] = m.machines.clone().into();
                }
                if !m.colors.is_empty() {
                    l["machineColors"] = serde_json::to_value(&m.colors)?;
                }
                doc[LAYOUT_KEY] = l;
            }
            crate::fsutil::write_atomic(&self.path, &serde_json::to_vec(&doc)?)
        };
        if let Err(e) = write() {
            tracing::warn!("{}: {e}", self.path.display());
        }
    }
}

/// Drops the oldest by `updated` (lowest id text on a tie) until the cap holds.
fn evict(m: &mut BTreeMap<String, Entry>) {
    crate::fsutil::evict_oldest(m, MAX_ENTRIES, |id, e| (e.updated, id.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn clean_home_order_keeps_valid_ids_in_order() {
        assert_eq!(
            clean_home_order(v(&["host", "resume", "library", "x-1"])),
            v(&["host", "resume", "library", "x-1"])
        );
    }

    #[test]
    fn clean_home_order_drops_bad_charset_and_lengths() {
        let long = "a".repeat(HOME_ID_MAX + 1);
        let max = "a".repeat(HOME_ID_MAX);
        let got = clean_home_order(v(&[
            "", "Host", "ho st", "h/st", "<b>", "é", "a_b", &long, &max, "ok",
        ]));
        assert_eq!(got, v(&[&max, "ok"]));
    }

    #[test]
    fn clean_home_order_dedupes_keeping_the_first() {
        assert_eq!(
            clean_home_order(v(&["a", "b", "a", "c", "b"])),
            v(&["a", "b", "c"])
        );
    }

    #[test]
    fn clean_home_order_clamps_to_the_max_after_dropping_bad_ones() {
        let mut ids = vec!["BAD".to_string()];
        ids.extend((0..HOME_MAX + 10).map(|i| format!("c{i}")));
        let got = clean_home_order(ids);
        assert_eq!(got.len(), HOME_MAX);
        assert_eq!(got[0], "c0");
        assert_eq!(got[HOME_MAX - 1], format!("c{}", HOME_MAX - 1));
    }

    #[test]
    fn load_reads_v1_layout_and_ignores_other_versions() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("prefs.json");
        std::fs::write(&p, r#"{"1":{"preset":"fast","codec":"h264","updated":1},"layout":{"v":1,"homeOrder":["recent","host","recent","BAD"]}}"#).unwrap();
        let prefs = Prefs::load(p.clone());
        assert_eq!(prefs.home_order(), v(&["recent", "host"]));
        assert_eq!(prefs.all().len(), 1);
        for raw in [
            r#"{"layout":{"v":2,"homeOrder":["host"]}}"#,
            r#"{"layout":{"homeOrder":["host"]}}"#,
            r#"{"layout":{"v":1,"homeOrder":"host"}}"#,
            r#"{"layout":[1,2]}"#,
        ] {
            std::fs::write(&p, raw).unwrap();
            assert!(Prefs::load(p.clone()).home_order().is_empty(), "{raw}");
        }
    }

    #[test]
    fn home_order_roundtrips_through_the_file_and_clears() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("prefs.json");
        let prefs = Prefs::load(p.clone());
        assert_eq!(
            prefs
                .set_layout(Some(v(&["b", "a", "b"])), None, None)
                .unwrap()
                .0,
            v(&["b", "a"])
        );
        assert_eq!(Prefs::load(p.clone()).home_order(), v(&["b", "a"]));
        prefs.set_layout(Some(vec![]), None, None).unwrap();
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert!(raw.get("layout").is_none(), "{raw}");
        assert!(Prefs::load(p).home_order().is_empty());
    }

    #[test]
    fn clean_machines_order_rules_and_cap() {
        let long = "a".repeat(HOME_ID_MAX + 1);
        assert_eq!(
            clean_machines_order(v(&["", "Up", "a b", &long, "vm-1", "x", "vm-1"])),
            v(&["vm-1", "x"])
        );
        let got = clean_machines_order((0..MACHINES_MAX + 5).map(|i| format!("m{i}")).collect());
        assert_eq!(got.len(), MACHINES_MAX);
    }

    #[test]
    fn machines_order_roundtrips_and_partial_set_keeps_the_other() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("prefs.json");
        let prefs = Prefs::load(p.clone());
        prefs
            .set_layout(Some(v(&["h"])), Some(v(&["b", "a", "b", "BAD"])), None)
            .unwrap();
        let l = Prefs::load(p.clone());
        assert_eq!(
            (l.home_order(), l.machines_order()),
            (v(&["h"]), v(&["b", "a"]))
        );
        let (h, m, _) = prefs.set_layout(None, Some(v(&["z"])), None).unwrap();
        assert_eq!((h, m), (v(&["h"]), v(&["z"])));
        let (h, m, _) = prefs.set_layout(Some(vec![]), None, None).unwrap();
        assert_eq!((h, m), (vec![], v(&["z"])));
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(
            raw["layout"],
            serde_json::json!({"v":1,"machinesOrder":["z"]})
        );
        prefs.set_layout(None, Some(vec![]), None).unwrap();
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert!(raw.get("layout").is_none(), "{raw}");
    }

    #[test]
    fn old_and_corrupt_layouts_read_without_panic() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("prefs.json");
        for (raw, home, machines) in [
            (r#"{"layout":{"v":1,"homeOrder":["a"]}}"#, v(&["a"]), v(&[])),
            (
                r#"{"layout":{"v":1,"machinesOrder":["m","m"]}}"#,
                v(&[]),
                v(&["m"]),
            ),
            (
                r#"{"layout":{"v":1,"homeOrder":["a"],"machinesOrder":7}}"#,
                v(&["a"]),
                v(&[]),
            ),
            (
                r#"{"layout":{"v":2,"machinesOrder":["m"]}}"#,
                v(&[]),
                v(&[]),
            ),
            (
                r#"{"layout":{"v":1,"machinesOrder":[1,null]}}"#,
                v(&[]),
                v(&[]),
            ),
            ("not json {", v(&[]), v(&[])),
        ] {
            std::fs::write(&p, raw).unwrap();
            let l = Prefs::load(p.clone());
            assert_eq!(
                (l.home_order(), l.machines_order()),
                (home, machines),
                "{raw}"
            );
        }
    }

    fn cm(pairs: &[(&str, Option<&str>)]) -> Option<BTreeMap<String, Option<String>>> {
        Some(
            pairs
                .iter()
                .map(|(k, c)| (k.to_string(), c.map(String::from)))
                .collect(),
        )
    }

    #[test]
    fn normalize_color_table() {
        for (i, o) in [
            ("#FFF", Some("#ffffff")),
            ("#AbC123", Some("#abc123")),
            ("#a1b", Some("#aa11bb")),
            ("#12345", None),
            ("#1234", None),
            ("#1234567", None),
            ("#", None),
            ("ffffff", None),
            ("fff", None),
            ("red", None),
            ("#ggg", None),
            ("#ff ", None),
            ("javascript:alert(1)", None),
            ("#fff;x", None),
            ("", None),
        ] {
            assert_eq!(normalize_color(i).as_deref(), o, "{i}");
        }
    }

    #[test]
    fn colors_merge_remove_reject_atomically_and_cap() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("prefs.json");
        let prefs = Prefs::load(p.clone());
        prefs
            .set_layout(
                None,
                None,
                cm(&[("a", Some("#FFF")), ("b", Some("#000000"))]),
            )
            .unwrap();
        prefs
            .set_layout(
                None,
                None,
                cm(&[("b", None), ("zz", None), ("c", Some("#123"))]),
            )
            .unwrap();
        let want: BTreeMap<String, String> = [("a", "#ffffff"), ("c", "#112233")]
            .map(|(k, c)| (k.into(), c.into()))
            .into();
        assert_eq!(prefs.machine_colors(), want);
        assert_eq!(Prefs::load(p.clone()).machine_colors(), want);
        // a bad id or colour anywhere stores nothing
        for bad in [("Bad", "#fff"), ("a b", "#fff"), ("", "#fff"), ("d", "red")] {
            assert!(
                prefs
                    .set_layout(
                        None,
                        None,
                        cm(&[("d2", Some("#000")), (bad.0, Some(bad.1))])
                    )
                    .is_err()
            );
        }
        assert_eq!(prefs.machine_colors(), want);
        // the cap counts the merged map
        let many: Vec<(String, Option<&str>)> = (0..COLORS_MAX)
            .map(|i| (format!("m{i}"), Some("#000")))
            .collect();
        let many: Vec<(&str, Option<&str>)> = many.iter().map(|(k, c)| (k.as_str(), *c)).collect();
        assert!(prefs.set_layout(None, None, cm(&many)).is_err());
        assert_eq!(prefs.machine_colors(), want);
        prefs
            .set_layout(None, None, cm(&many[..COLORS_MAX - 2]))
            .unwrap();
        assert_eq!(prefs.machine_colors().len(), COLORS_MAX);
        // empty map leaves no key; clearing everything leaves no layout
        let ids: Vec<(String, Option<&str>)> = prefs
            .machine_colors()
            .into_keys()
            .map(|k| (k, None))
            .collect();
        let ids: Vec<(&str, Option<&str>)> = ids.iter().map(|(k, c)| (k.as_str(), *c)).collect();
        prefs.set_layout(None, Some(v(&["x"])), cm(&ids)).unwrap();
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(
            raw["layout"],
            serde_json::json!({"v":1,"machinesOrder":["x"]})
        );
    }

    #[test]
    fn colors_load_leniently() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("prefs.json");
        let big: BTreeMap<String, String> = (0..COLORS_MAX + 5)
            .map(|i| (format!("m{i:03}"), "#000".into()))
            .collect();
        let big = serde_json::json!({"layout":{"v":1,"machineColors":big}}).to_string();
        for (raw, n) in [
            (r#"{"layout":{"v":1,"homeOrder":["a"]}}"#.to_string(), 0),
            (r##"{"layout":{"v":1,"machineColors":7}}"##.to_string(), 0),
            (r##"{"layout":{"v":1,"machineColors":["#fff"]}}"##.to_string(), 0),
            (r##"{"layout":{"v":2,"machineColors":{"a":"#fff"}}}"##.to_string(), 0),
            (r##"{"layout":{"v":1,"machineColors":{"a":"#FFF","B":"#fff","c":"red","d":5,"e":null}}}"##.to_string(), 1),
            (big, COLORS_MAX),
            ("not json {".to_string(), 0),
        ] {
            std::fs::write(&p, &raw).unwrap();
            assert_eq!(Prefs::load(p.clone()).machine_colors().len(), n, "{raw}");
        }
    }
}
