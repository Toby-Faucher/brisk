//! Per-game quality in `data_dir/prefs.json` (`{"<appid>": Entry}`): written whole to a 0600 temp
//! file and renamed over the old one, read tolerantly (a missing or corrupt file is just empty).
//! The Home card order and the Machines page order ride along under
//! `"layout": {"v": 1, "homeOrder": [...], "machinesOrder": [...]}` (each key absent when empty).

use crate::sessions::now_ms;
use crate::stream::{Codec, Preset};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

pub const MAX_ENTRIES: usize = 500;
/// Home cards are a handful; the caps only bound what a client can make hostd store.
pub const HOME_MAX: usize = 32;
pub const HOME_ID_MAX: usize = 32;
pub const MACHINES_MAX: usize = 64;
const LAYOUT_KEY: &str = "layout";
const LAYOUT_V: u64 = 1;

/// Keeps the ids that are 1..=HOME_ID_MAX of `a-z 0-9 -`, first occurrence only, at most HOME_MAX.
pub fn clean_home_order(ids: Vec<String>) -> Vec<String> {
    clean_ids(ids, HOME_MAX)
}

/// Same rules as `clean_home_order`, at most MACHINES_MAX ids.
pub fn clean_machines_order(ids: Vec<String>) -> Vec<String> {
    clean_ids(ids, MACHINES_MAX)
}

fn clean_ids(ids: Vec<String>, max: usize) -> Vec<String> {
    let ok = |s: &str| {
        (1..=HOME_ID_MAX).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if out.len() == max {
            break;
        }
        if ok(&id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

#[derive(Default)]
struct Inner {
    map: BTreeMap<u32, Entry>,
    home: Vec<String>,
    machines: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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

pub struct Prefs {
    path: PathBuf,
    inner: Mutex<Inner>,
}

impl Prefs {
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
        let mut map: BTreeMap<u32, Entry> = raw
            .into_iter()
            .filter_map(|(k, v)| Some((k.parse().ok()?, serde_json::from_value(v).ok()?)))
            .collect();
        evict(&mut map);
        Prefs {
            path,
            inner: Mutex::new(Inner {
                map,
                home,
                machines,
            }),
        }
    }

    pub fn all(&self) -> BTreeMap<u32, Entry> {
        self.inner.lock().unwrap().map.clone()
    }

    pub fn home_order(&self) -> Vec<String> {
        self.inner.lock().unwrap().home.clone()
    }

    pub fn machines_order(&self) -> Vec<String> {
        self.inner.lock().unwrap().machines.clone()
    }

    /// Cleans, stores and writes the lists that are `Some` (the other keeps its value); an empty
    /// list clears that order. Returns (home, machines) as stored.
    pub fn set_layout(
        &self,
        home: Option<Vec<String>>,
        machines: Option<Vec<String>>,
    ) -> (Vec<String>, Vec<String>) {
        let mut m = self.inner.lock().unwrap();
        if let Some(h) = home {
            m.home = clean_home_order(h);
        }
        if let Some(x) = machines {
            m.machines = clean_machines_order(x);
        }
        self.save(&m);
        (m.home.clone(), m.machines.clone())
    }

    /// Stores `e` with its `updated` as given and writes the file.
    pub fn put_entry(&self, appid: u32, e: Entry) {
        let mut m = self.inner.lock().unwrap();
        m.map.insert(appid, e);
        evict(&mut m.map);
        self.save(&m);
    }

    /// Like `put_entry` with `updated` = now, but always newer than every entry already there.
    pub fn put(&self, appid: u32, mut e: Entry) -> Entry {
        let mut m = self.inner.lock().unwrap();
        let newest = m.map.values().map(|e| e.updated).max().unwrap_or(0);
        e.updated = now_ms().max(newest + 1);
        m.map.insert(appid, e.clone());
        evict(&mut m.map);
        self.save(&m);
        e
    }

    pub fn delete(&self, appid: u32) {
        let mut m = self.inner.lock().unwrap();
        if m.map.remove(&appid).is_some() {
            self.save(&m);
        }
    }

    /// A failed write is a warning: the change then lives in memory until hostd exits.
    fn save(&self, m: &Inner) {
        let write = || -> std::io::Result<()> {
            let mut doc = serde_json::to_value(&m.map)?;
            if !m.home.is_empty() || !m.machines.is_empty() {
                let mut l = serde_json::json!({ "v": LAYOUT_V });
                if !m.home.is_empty() {
                    l["homeOrder"] = m.home.clone().into();
                }
                if !m.machines.is_empty() {
                    l["machinesOrder"] = m.machines.clone().into();
                }
                doc[LAYOUT_KEY] = l;
            }
            let dir = self.path.parent().unwrap_or(std::path::Path::new("."));
            std::fs::create_dir_all(dir)?;
            let tmp = dir.join(format!(".prefs.json.{}.tmp", std::process::id()));
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            let r = f
                .write_all(&serde_json::to_vec(&doc)?)
                .and_then(|_| f.sync_all())
                .and_then(|_| std::fs::rename(&tmp, &self.path));
            if r.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            r
        };
        if let Err(e) = write() {
            eprintln!("warning: {}: {e}", self.path.display());
        }
    }
}

/// Drops the oldest by `updated` (lowest appid on a tie) until the cap holds.
fn evict(m: &mut BTreeMap<u32, Entry>) {
    while m.len() > MAX_ENTRIES {
        let oldest = m
            .iter()
            .min_by_key(|(id, e)| (e.updated, **id))
            .map(|(id, _)| *id);
        m.remove(&oldest.unwrap());
    }
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
            prefs.set_layout(Some(v(&["b", "a", "b"])), None).0,
            v(&["b", "a"])
        );
        assert_eq!(Prefs::load(p.clone()).home_order(), v(&["b", "a"]));
        prefs.set_layout(Some(vec![]), None);
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
        prefs.set_layout(Some(v(&["h"])), Some(v(&["b", "a", "b", "BAD"])));
        let l = Prefs::load(p.clone());
        assert_eq!(
            (l.home_order(), l.machines_order()),
            (v(&["h"]), v(&["b", "a"]))
        );
        let (h, m) = prefs.set_layout(None, Some(v(&["z"])));
        assert_eq!((h, m), (v(&["h"]), v(&["z"])));
        let (h, m) = prefs.set_layout(Some(vec![]), None);
        assert_eq!((h, m), (vec![], v(&["z"])));
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(
            raw["layout"],
            serde_json::json!({"v":1,"machinesOrder":["z"]})
        );
        prefs.set_layout(None, Some(vec![]));
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
}
