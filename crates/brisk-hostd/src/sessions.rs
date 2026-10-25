//! Stream history in `data_dir/sessions.jsonl`: one line when a session opens (`ended: null`) and
//! one when it closes; a later line with the same `started` replaces the earlier one.

use crate::stream::Preset;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionRec {
    pub appid: Option<u32>,
    pub name: String,
    /// Unix milliseconds.
    pub started: u64,
    pub ended: Option<u64>,
    pub host: String,
    pub preset: Preset,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

pub struct Sessions {
    pub path: PathBuf,
}

impl Sessions {
    pub fn append(&self, r: &SessionRec) {
        let write = || -> std::io::Result<()> {
            if let Some(d) = self.path.parent() {
                std::fs::create_dir_all(d)?;
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            writeln!(f, "{}", serde_json::to_string(r)?)
        };
        if let Err(e) = write() {
            eprintln!("warning: {}: {e}", self.path.display());
        }
    }

    pub fn close(&self, mut r: SessionRec) {
        r.ended = Some(now_ms().max(r.started));
        self.append(&r);
    }

    /// Newest first. shortcut: reads the whole file each time, fine for years of play; rotate if it grows past MBs.
    pub fn latest(&self, n: usize) -> Vec<SessionRec> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let mut out: Vec<SessionRec> = vec![];
        for r in text
            .lines()
            .filter_map(|l| serde_json::from_str::<SessionRec>(l).ok())
        {
            match out.iter_mut().find(|o| o.started == r.started) {
                Some(o) => *o = r,
                None => out.push(r),
            }
        }
        out.sort_by_key(|r| std::cmp::Reverse(r.started));
        out.truncate(n);
        out
    }
}
