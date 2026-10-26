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

/// Points per session (5 s each: 24 h) and series files kept (newest sessions).
pub const MAX_POINTS: usize = 17_280;
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

/// Fills `f` only with a parsed value: a later garbled line never clears an earlier one.
fn set<T>(f: &mut Option<T>, v: Option<T>) {
    if v.is_some() {
        *f = v;
    }
}

/// Reads the bridge's stdout: nothing counts before the report's first line (`tap_frames`), so a
/// stray earlier line with a known key cannot fill a field.
#[derive(Default)]
pub struct Report {
    on: bool,
    pub stats: Stats,
}

impl Report {
    /// One stdout line of the bridge (with or without hostd's `bridge\t` prefix); others are ignored.
    pub fn line(&mut self, l: &str) {
        let l = l.strip_prefix("bridge\t").unwrap_or(l);
        let Some((key, rest)) = l.split_once('\t') else {
            return;
        };
        self.on |= key == "tap_frames";
        if !self.on {
            return;
        }
        let s = &mut self.stats;
        match key {
            "tap_frames" => set(&mut s.frames, rest.trim().parse().ok()),
            "browser_leg" => {
                set(&mut s.sent, after(rest, "sent"));
                set(&mut s.dropped, after(rest, "dropped"));
                set(&mut s.resets, after(rest, "reset"));
                set(&mut s.idr_requests, after(rest, "idr_requests"));
            }
            "bridge_write-capture" => {
                set(&mut s.bridge_p50, ms(rest, "p50"));
                set(&mut s.bridge_p99, ms(rest, "p99"));
            }
            "frame_processing(tap-capture)" => {
                set(&mut s.host_p50, ms(rest, "p50"));
                set(&mut s.host_p99, ms(rest, "p99"));
            }
            "audio" => {
                set(&mut s.audio_lost, after(rest, "lost"));
                set(&mut s.audio_errors, after(rest, "errors"));
            }
            "input" => {
                set(&mut s.input_received, after(rest, "received"));
                set(&mut s.input_sent, after(rest, "sent"));
                set(&mut s.input_dropped, after(rest, "dropped"));
            }
            "session" => set(
                &mut s.duration_s,
                rest.strip_prefix("ended after ")
                    .and_then(|s| s.trim().strip_suffix('s')?.parse().ok())
                    .filter(|v: &f64| v.is_finite()),
            ),
            k if k.starts_with("gate(") => set(
                &mut s.gate,
                match rest.trim() {
                    "PASS" => Some(Gate::Pass),
                    "FAIL" => Some(Gate::Fail),
                    _ => None,
                },
            ),
            _ => {}
        }
    }
}

impl Stats {
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

    /// The last line of that session: one pass from the end, stopping at it.
    pub fn get(&self, started: u64) -> Option<SessionRec> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        text.lines()
            .rev()
            .filter_map(|l| serde_json::from_str::<SessionRec>(l).ok())
            .find(|r| r.started == started)
    }

    /// Up to `n` mean-binned delays of the session's series (the list's sparkline); empty without one.
    pub fn spark(&self, started: u64, n: usize) -> Vec<f32> {
        let s = self.series(started);
        if s.len() < 2 {
            return vec![];
        }
        let bins = n.min(s.len());
        (0..bins)
            .map(|i| {
                let b = &s[i * s.len() / bins..(i + 1) * s.len() / bins];
                (b.iter().map(|p| p[1]).sum::<f64>() / b.len() as f64) as f32
            })
            .collect()
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
        let _g = self.series_lock.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut ids: Vec<u64> = vec![];
        for e in dir.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            // Called under series_lock, so a `.tmp` here is a leftover of a crashed write.
            if name.starts_with('.') && name.ends_with(".tmp") {
                let _ = std::fs::remove_file(e.path());
            } else if let Some(id) = name
                .strip_suffix(".series.json")
                .and_then(|i| i.parse().ok())
            {
                ids.push(id);
            }
        }
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
        let mut r = Report::default();
        for l in text.lines() {
            r.line(l);
        }
        r.stats
    }

    /// Lines before the report's header are not the report; a garbled repeat keeps the parsed value.
    #[test]
    fn report_starts_at_tap_frames_and_keeps_parsed_values() {
        let s = parse(
            "input\treceived 9 sent 9 dropped 9\nsession\tended after 1.0s\n\
             tap_frames\t10\ninput\treceived 5 sent 4 dropped 1\ninput\tgarbage\n\
             session\tended after 2.5s",
        );
        assert_eq!(s.frames, Some(10));
        assert_eq!(
            (s.input_received, s.input_sent, s.input_dropped),
            (Some(5), Some(4), Some(1))
        );
        assert_eq!(s.duration_s, Some(2.5));
        assert_eq!(parse("browser_leg\tsent 5 dropped 1"), Stats::default());
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
        assert_eq!(
            parse("tap_frames\t1\nbridge\tgate(x)\tFAIL").gate,
            Some(Gate::Fail)
        );
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
