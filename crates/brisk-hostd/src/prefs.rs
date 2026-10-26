//! Per-game quality in `data_dir/prefs.json` (`{"<appid>": Entry}`): written whole to a 0600 temp
//! file and renamed over the old one, read tolerantly (a missing or corrupt file is just empty).

use crate::sessions::now_ms;
use crate::stream::{Codec, Preset};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

pub const MAX_ENTRIES: usize = 500;

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
    map: Mutex<BTreeMap<u32, Entry>>,
}

impl Prefs {
    pub fn load(path: PathBuf) -> Prefs {
        let raw: BTreeMap<String, serde_json::Value> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut map: BTreeMap<u32, Entry> = raw
            .into_iter()
            .filter_map(|(k, v)| Some((k.parse().ok()?, serde_json::from_value(v).ok()?)))
            .collect();
        evict(&mut map);
        Prefs {
            path,
            map: Mutex::new(map),
        }
    }

    pub fn all(&self) -> BTreeMap<u32, Entry> {
        self.map.lock().unwrap().clone()
    }

    /// Stores `e` with its `updated` as given and writes the file.
    pub fn put_entry(&self, appid: u32, e: Entry) {
        let mut m = self.map.lock().unwrap();
        m.insert(appid, e);
        evict(&mut m);
        self.save(&m);
    }

    /// Like `put_entry` with `updated` = now, but always newer than every entry already there.
    pub fn put(&self, appid: u32, mut e: Entry) -> Entry {
        let mut m = self.map.lock().unwrap();
        let newest = m.values().map(|e| e.updated).max().unwrap_or(0);
        e.updated = now_ms().max(newest + 1);
        m.insert(appid, e.clone());
        evict(&mut m);
        self.save(&m);
        e
    }

    pub fn delete(&self, appid: u32) {
        let mut m = self.map.lock().unwrap();
        if m.remove(&appid).is_some() {
            self.save(&m);
        }
    }

    /// A failed write is a warning: the change then lives in memory until hostd exits.
    fn save(&self, m: &BTreeMap<u32, Entry>) {
        let write = || -> std::io::Result<()> {
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
                .write_all(&serde_json::to_vec(m)?)
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
