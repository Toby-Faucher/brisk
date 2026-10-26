//! Stream history in `data_dir/sessions.jsonl`: one line when a session opens (`ended: null`) and
//! one when it closes; a later line with the same `started` replaces the earlier one. The page's
//! delay/fps/bitrate samples live beside it in `data_dir/sessions/<started>.series.json`.

use crate::stream::{Codec, Preset};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Points per session (5 s each: a bit over 4 h) and series files kept (newest sessions).
pub const MAX_POINTS: usize = 3000;
pub const MAX_SERIES: usize = 200;

/// `[t seconds since started, delay ms, fps, Mbit/s]`.
pub type Point = [f64; 4];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRec {
    pub appid: Option<u32>,
    pub name: String,
    /// Unix milliseconds.
    pub started: u64,
    pub ended: Option<u64>,
    pub host: String,
    /// Preset, codec and bitrate are the last ones of the session (a quality change updates them).
    pub preset: Preset,
    #[serde(default)]
    pub codec: Option<Codec>,
    #[serde(default)]
    pub bitrate_kbps: Option<u32>,
    /// From the bridge's end-of-session report; null for old records or a bridge that printed none.
    #[serde(default)]
    pub stats: Option<Stats>,
}

impl SessionRec {
    /// Adds one bridge's report (a quality change restarts the bridge, so a session can have several).
    pub fn add_stats(&mut self, s: Stats) {
        if s != Stats::default() {
            self.stats = Some(match self.stats.take() {
                Some(a) => a.merge(s),
                None => s,
            });
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Gate {
    Pass,
    Fail,
}

/// The bridge's end-of-session report (docs/m0-baseline.md); a missing or garbled line leaves its fields None.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub duration_s: Option<f64>,
    pub frames: Option<u64>,
    pub sent: Option<u64>,
    pub dropped: Option<u64>,
    pub resets: Option<u64>,
    pub idr_requests: Option<u64>,
    /// `bridge_write-capture`, ms.
    pub bridge_p50: Option<f64>,
    pub bridge_p99: Option<f64>,
    /// `frame_processing(tap-capture)` (Sunshine's own), ms.
    pub host_p50: Option<f64>,
    pub host_p99: Option<f64>,
    pub audio_lost: Option<u64>,
    pub audio_errors: Option<u64>,
    pub input_received: Option<u64>,
    pub input_sent: Option<u64>,
    pub input_dropped: Option<u64>,
    pub gate: Option<Gate>,
}

/// The value after the word `key` in `rest` (`sent 5 dropped 1`).
fn after<T: std::str::FromStr>(rest: &str, key: &str) -> Option<T> {
    let mut w = rest.split_whitespace();
    w.by_ref().find(|x| *x == key)?;
    w.next()?.parse().ok()
}

/// Like `after`, finite only (Rust parses "NaN" and "inf").
fn ms(rest: &str, key: &str) -> Option<f64> {
    after::<f64>(rest, key).filter(|v| v.is_finite())
}

fn sum<T: std::ops::Add<Output = T>>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (x, y) => x.or(y),
    }
}

impl Stats {
    /// One stdout line of the bridge (with or without hostd's `bridge\t` prefix); others are ignored.
    pub fn line(&mut self, l: &str) {
        let l = l.strip_prefix("bridge\t").unwrap_or(l);
        let Some((key, rest)) = l.split_once('\t') else {
            return;
        };
        match key {
            "tap_frames" => self.frames = rest.trim().parse().ok(),
            "browser_leg" => {
                self.sent = after(rest, "sent");
                self.dropped = after(rest, "dropped");
                self.resets = after(rest, "reset");
                self.idr_requests = after(rest, "idr_requests");
            }
            "bridge_write-capture" => {
                self.bridge_p50 = ms(rest, "p50");
                self.bridge_p99 = ms(rest, "p99");
            }
            "frame_processing(tap-capture)" => {
                self.host_p50 = ms(rest, "p50");
                self.host_p99 = ms(rest, "p99");
            }
            "audio" => {
                self.audio_lost = after(rest, "lost");
                self.audio_errors = after(rest, "errors");
            }
            "input" => {
                self.input_received = after(rest, "received");
                self.input_sent = after(rest, "sent");
                self.input_dropped = after(rest, "dropped");
            }
            "session" => {
                self.duration_s = rest
                    .strip_prefix("ended after ")
                    .and_then(|s| s.trim().strip_suffix('s')?.parse().ok())
                    .filter(|v: &f64| v.is_finite());
            }
            k if k.starts_with("gate(") => {
                self.gate = match rest.trim() {
                    "PASS" => Some(Gate::Pass),
                    "FAIL" => Some(Gate::Fail),
                    _ => None,
                }
            }
            _ => {}
        }
    }

    /// `self` then `b`: counts add up, p50 is frame-weighted, p99 the worst, one FAIL fails the session.
    pub fn merge(self, b: Stats) -> Stats {
        let p50 = |x: Option<f64>, y: Option<f64>| match (x, y, self.frames, b.frames) {
            (Some(x), Some(y), Some(fx), Some(fy)) if fx + fy > 0 => {
                Some((x * fx as f64 + y * fy as f64) / (fx + fy) as f64)
            }
            _ => y.or(x),
        };
        let p99 = |x: Option<f64>, y: Option<f64>| match (x, y) {
            (Some(x), Some(y)) => Some(x.max(y)),
            _ => x.or(y),
        };
        Stats {
            duration_s: sum(self.duration_s, b.duration_s),
            frames: sum(self.frames, b.frames),
            sent: sum(self.sent, b.sent),
            dropped: sum(self.dropped, b.dropped),
            resets: sum(self.resets, b.resets),
            idr_requests: sum(self.idr_requests, b.idr_requests),
            bridge_p50: p50(self.bridge_p50, b.bridge_p50),
            bridge_p99: p99(self.bridge_p99, b.bridge_p99),
            host_p50: p50(self.host_p50, b.host_p50),
            host_p99: p99(self.host_p99, b.host_p99),
            audio_lost: sum(self.audio_lost, b.audio_lost),
            audio_errors: sum(self.audio_errors, b.audio_errors),
            input_received: sum(self.input_received, b.input_received),
            input_sent: sum(self.input_sent, b.input_sent),
            input_dropped: sum(self.input_dropped, b.input_dropped),
            gate: match (self.gate, b.gate) {
                (Some(Gate::Fail), _) | (_, Some(Gate::Fail)) => Some(Gate::Fail),
                (x, y) => y.or(x),
            },
        }
    }
}

/// Finite and plausible: t within a week, delay under a minute, fps up to 1000, up to 10 Gbit/s.
pub fn valid_point(p: &Point) -> bool {
    let max = [604_800.0, 60_000.0, 1000.0, 10_000.0];
    p.iter()
        .zip(max)
        .all(|(v, m)| v.is_finite() && (0.0..=m).contains(v))
}

#[derive(Debug, PartialEq)]
pub enum SeriesError {
    NoSession,
    TooMany,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

pub struct Sessions {
    pub path: PathBuf,
    /// Serializes series read-merge-writes.
    series_lock: Mutex<()>,
}

impl Sessions {
    pub fn new(path: PathBuf) -> Sessions {
        Sessions {
            path,
            series_lock: Mutex::new(()),
        }
    }

    pub fn get(&self, started: u64) -> Option<SessionRec> {
        self.latest(usize::MAX)
            .into_iter()
            .find(|r| r.started == started)
    }

    fn series_dir(&self) -> PathBuf {
        self.path.with_file_name("sessions")
    }

    fn series_path(&self, started: u64) -> PathBuf {
        self.series_dir().join(format!("{started}.series.json"))
    }

    /// Sorted by t; a missing or corrupt file is empty.
    pub fn series(&self, started: u64) -> Vec<Point> {
        std::fs::read(self.series_path(started))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Merges `pts` (validated by the caller) into the session's series, a later point replacing one
    /// with the same t. Refuses past MAX_POINTS (nothing is written then); drops the series files of
    /// all but the newest MAX_SERIES sessions.
    pub fn put_series(&self, started: u64, pts: &[Point]) -> Result<(), SeriesError> {
        if self.get(started).is_none() {
            return Err(SeriesError::NoSession);
        }
        let _g = self.series_lock.lock().unwrap();
        // Keyed by the bits of t: valid t are finite and >= 0 (+ 0.0 folds -0), so bit order is numeric.
        let key = |p: &Point| (p[0] + 0.0).to_bits();
        let mut m: BTreeMap<u64, Point> = self
            .series(started)
            .into_iter()
            .map(|p| (key(&p), p))
            .collect();
        m.extend(pts.iter().map(|p| (key(p), *p)));
        if m.len() > MAX_POINTS {
            return Err(SeriesError::TooMany);
        }
        let all: Vec<Point> = m.into_values().collect();
        let path = self.series_path(started);
        let write = || -> std::io::Result<()> {
            let dir = self.series_dir();
            std::fs::create_dir_all(&dir)?;
            let tmp = dir.join(format!(".{started}.{}.tmp", std::process::id()));
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            let r = f
                .write_all(&serde_json::to_vec(&all)?)
                .and_then(|_| std::fs::rename(&tmp, &path));
            if r.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            r
        };
        if let Err(e) = write() {
            eprintln!("warning: {}: {e}", path.display());
        }
        self.evict_series();
        Ok(())
    }

    fn evict_series(&self) {
        let Ok(dir) = std::fs::read_dir(self.series_dir()) else {
            return;
        };
        let mut ids: Vec<u64> = dir
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_str()?
                    .strip_suffix(".series.json")?
                    .parse()
                    .ok()
            })
            .collect();
        ids.sort_unstable();
        let over = ids.len().saturating_sub(MAX_SERIES);
        for id in &ids[..over] {
            let _ = std::fs::remove_file(self.series_path(*id));
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Stats {
        let mut s = Stats::default();
        for l in text.lines() {
            s.line(l);
        }
        s
    }

    /// The end-of-session block of a real bawn run (brisk-up log, `bridge\t` prefix and all).
    #[test]
    fn parses_real_report() {
        let s = parse(include_str!("../tests/fixtures/bridge-report.txt"));
        assert_eq!(
            s,
            Stats {
                duration_s: Some(970.2),
                frames: Some(58193),
                sent: Some(58181),
                dropped: Some(11),
                resets: Some(0),
                idr_requests: Some(2),
                bridge_p50: Some(4.3),
                bridge_p99: Some(8.7),
                host_p50: Some(4.2),
                host_p99: Some(8.6),
                audio_lost: Some(0),
                audio_errors: Some(0),
                input_received: Some(48138),
                input_sent: Some(46605),
                input_dropped: Some(0),
                gate: Some(Gate::Pass),
            }
        );
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["durationS"], 970.2);
        assert_eq!(j["idrRequests"], 2);
        assert_eq!(j["gate"], "pass");
    }

    /// Cut-off, garbled and unknown lines leave fields empty instead of failing.
    #[test]
    fn garbled_report_is_tolerated() {
        let s = parse(include_str!("../tests/fixtures/bridge-report-garbled.txt"));
        assert_eq!(
            s,
            Stats {
                sent: Some(100),
                host_p50: Some(3.1),
                audio_lost: Some(2),
                input_received: Some(5),
                input_dropped: Some(1),
                ..Stats::default()
            }
        );
        assert_eq!(parse("bridge\tgate(x)\tFAIL").gate, Some(Gate::Fail));
        assert_eq!(parse(""), Stats::default());
    }

    /// A quality change restarts the bridge: its reports add up into one session.
    #[test]
    fn stats_of_two_bridges_merge() {
        let a = Stats {
            duration_s: Some(10.0),
            frames: Some(100),
            dropped: Some(1),
            bridge_p50: Some(4.0),
            bridge_p99: Some(9.0),
            gate: Some(Gate::Fail),
            ..Stats::default()
        };
        let b = Stats {
            duration_s: Some(30.0),
            frames: Some(300),
            dropped: Some(2),
            bridge_p50: Some(8.0),
            bridge_p99: Some(7.0),
            host_p50: Some(3.0),
            gate: Some(Gate::Pass),
            ..Stats::default()
        };
        let m = a.clone().merge(b);
        assert_eq!(m.duration_s, Some(40.0));
        assert_eq!(m.frames, Some(400));
        assert_eq!(m.dropped, Some(3));
        assert_eq!(m.bridge_p50, Some(7.0)); // frame-weighted
        assert_eq!(m.bridge_p99, Some(9.0)); // worst
        assert_eq!(m.host_p50, Some(3.0));
        assert_eq!(m.gate, Some(Gate::Fail));

        let mut r: SessionRec = serde_json::from_str(
            r#"{"appid":null,"name":"Desktop","started":1,"ended":null,"host":"bawn","preset":"fast"}"#,
        )
        .unwrap();
        assert_eq!(
            (r.stats.is_none(), r.codec, r.bitrate_kbps),
            (true, None, None)
        );
        r.add_stats(Stats::default());
        assert!(r.stats.is_none(), "an empty report adds nothing");
        r.add_stats(a.clone());
        r.add_stats(Stats::default());
        assert_eq!(r.stats, Some(a));
    }

    #[test]
    fn point_bounds() {
        assert!(valid_point(&[0.0, 12.5, 60.0, 20.0]));
        for p in [
            [-1.0, 1.0, 1.0, 1.0],
            [1.0, f64::NAN, 1.0, 1.0],
            [1.0, 1.0, f64::INFINITY, 1.0],
            [1.0, 1.0, 1.0, 1e300],
            [1e12, 1.0, 1.0, 1.0],
        ] {
            assert!(!valid_point(&p), "{p:?}");
        }
    }
}
