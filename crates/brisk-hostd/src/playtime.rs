//! Playtime for emulator games (roadmap Stage 6a): time counts while the emulator process runs.
//! `data_dir/playtime.json` is `{"v":1,"games":{"<id>":{"folded":secs,"sessions":[[start,end],..]}}}`
//! in unix seconds. This file is the pure store; the watcher that feeds it is `watch` below.
//! Reading is tolerant (missing or corrupt = empty); writing is atomic and 0600.

use crate::emu::{self, Record};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FILE: &str = "playtime.json";
/// Sessions closer than this (end to next start) are one session: a quick restart.
pub const GAP_S: u64 = 30;
const MAX_SESSIONS: usize = 500;
const MAX_GAMES: usize = 500;
const TWO_WEEKS_S: u64 = 14 * 24 * 3600;
const FLUSH_S: u64 = 60;

#[derive(Default, Serialize, Deserialize, Debug, PartialEq)]
struct Entry {
    /// Seconds of sessions older than the cap, kept only as a total.
    folded: u64,
    sessions: Vec<(u64, u64)>,
}

#[derive(Default, Debug, PartialEq)]
pub struct Store {
    games: BTreeMap<String, Entry>,
}

impl Store {
    /// A missing or corrupt file is an empty store.
    pub fn load(data_dir: &Path) -> Store {
        #[derive(Deserialize)]
        struct File {
            games: BTreeMap<String, Entry>,
        }
        let games = std::fs::read(data_dir.join(FILE))
            .ok()
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
            .map_or_else(BTreeMap::new, |f| f.games);
        Store { games }
    }

    pub fn save(&self, data_dir: &Path) {
        let bytes = serde_json::to_vec(&serde_json::json!({"v": 1, "games": self.games}))
            .unwrap_or_default();
        let path = data_dir.join(FILE);
        if let Err(e) = emu::write_atomic(&path, &bytes) {
            eprintln!("warning: {}: {e}", path.display());
        }
    }

    /// Adds `[start, end]` for `id`. It joins the latest session when it starts within `GAP_S` of
    /// that session's end (or inside it), so seeing the same time twice never counts it twice.
    pub fn add(&mut self, id: &str, start: u64, end: u64) {
        let end = end.max(start);
        let e = self.games.entry(id.to_string()).or_default();
        match e.sessions.last_mut() {
            Some(last) if start <= last.1.saturating_add(GAP_S) && end >= last.0 => {
                last.0 = last.0.min(start);
                last.1 = last.1.max(end);
            }
            _ => e.sessions.push((start, end)),
        }
        if e.sessions.len() > MAX_SESSIONS {
            let extra = e.sessions.len() - MAX_SESSIONS;
            e.folded += e.sessions.drain(..extra).map(|(s, e)| e - s).sum::<u64>();
        }
        while self.games.len() > MAX_GAMES {
            let oldest = self
                .games
                .iter()
                .min_by_key(|(_, e)| e.last_end())
                .map(|(k, _)| k.clone());
            self.games.remove(&oldest.unwrap());
        }
    }

    pub fn total_minutes(&self, id: &str) -> u64 {
        self.games.get(id).map_or(0, |e| {
            (e.folded + e.sessions.iter().map(|(s, e)| e - s).sum::<u64>()) / 60
        })
    }

    /// Minutes played in the 14 days up to `now`, counting only the part of a session inside it.
    pub fn last_two_weeks_minutes(&self, id: &str, now: u64) -> u64 {
        let from = now.saturating_sub(TWO_WEEKS_S);
        self.games.get(id).map_or(0, |e| {
            e.sessions
                .iter()
                .map(|&(s, e)| e.min(now).saturating_sub(s.max(from)))
                .sum::<u64>()
                / 60
        })
    }

    /// End of the latest session.
    pub fn last_end(&self, id: &str) -> Option<u64> {
        self.games
            .get(id)
            .and_then(|e| e.sessions.last().map(|s| s.1))
    }
}

impl Entry {
    fn last_end(&self) -> u64 {
        self.sessions.last().map_or(0, |s| s.1)
    }
}

/// One poll: every library game running at `now` gets its session extended to `now`. Returns the
/// ids running. A session is never extended past the last time a poll saw the game running.
pub fn tick(store: &mut Store, recs: &[Record], proc: &Path, now: u64) -> HashSet<String> {
    let ps = crate::running::procs(proc);
    let mut up = HashSet::new();
    for r in recs {
        if !emu::pids(&ps, r).is_empty() {
            store.add(r.id.as_str(), now, now);
            up.insert(r.id.to_string());
        }
    }
    up
}

/// The poll interval from `BRISK_PLAYTIME_POLL_S`: default 15 s, at least 5 s.
pub fn poll_from(raw: Option<&str>) -> Duration {
    Duration::from_secs(raw.and_then(|v| v.trim().parse().ok()).unwrap_or(15).max(5))
}

/// Background watcher, never returns. `base` is the unix time at start; later times follow the
/// tokio clock, so tests run on paused time. The store is held in memory and written when the
/// running set changes, and at most every minute while a game runs. After a hostd restart the
/// stored session ends at the last write; a game still running starts a new one from now (joined
/// to the old one when within `GAP_S`), so a restart can lose up to a minute but never invents time.
pub async fn watch(data_dir: PathBuf, proc: PathBuf, every: Duration, base: u64) {
    let t0 = tokio::time::Instant::now();
    let mut store = Store::load(&data_dir);
    let mut up: HashSet<String> = HashSet::new();
    let mut flushed = base;
    let mut tick_at = tokio::time::interval(every);
    tick_at.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick_at.tick().await;
        let now = base + t0.elapsed().as_secs();
        let recs = emu::load(&data_dir);
        let now_up = tick(&mut store, &recs, &proc, now);
        if now_up != up || (!now_up.is_empty() && now - flushed >= FLUSH_S) {
            store.save(&data_dir);
            flushed = now;
            up = now_up;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 3600;

    #[test]
    fn sessions_merge_inside_the_gap_and_split_beyond_it() {
        // (second session start offset from first end, expected session count)
        for (gap, n) in [(0, 1), (GAP_S, 1), (GAP_S + 1, 2), (3600, 2)] {
            let mut s = Store::default();
            s.add("g", 1000, 1100);
            s.add("g", 1100 + gap, 1200 + gap);
            assert_eq!(s.games["g"].sessions.len(), n, "gap {gap}");
        }
        let mut s = Store::default();
        s.add("g", 1000, 1100);
        s.add("g", 1050, 1100); // overlap: never double counted
        s.add("g", 1100, 1100);
        assert_eq!(s.games["g"].sessions, vec![(1000, 1100)]);
        assert_eq!(s.total_minutes("g"), 1);
        assert_eq!(s.total_minutes("none"), 0);
    }

    #[test]
    fn two_week_window_counts_only_the_part_inside() {
        let now = 100 * DAY;
        let from = now - 14 * DAY;
        let mut s = Store::default();
        s.add("g", from - 600, from + 600); // straddles the start: 10 min inside
        s.add("g", now - 3600, now - 1800); // 30 min
        s.add("g", now - 100 * DAY, now - 99 * DAY); // far outside
        assert_eq!(s.last_two_weeks_minutes("g", now), 40);
        assert_eq!(s.total_minutes("g"), 20 + 30 + 24 * 60);
        // a session in the future (clock moved back) is clipped to now
        s.add("h", now - 60, now + 3600);
        assert_eq!(s.last_two_weeks_minutes("h", now), 1);
    }

    #[test]
    fn size_is_bounded_and_folding_keeps_the_total() {
        let mut s = Store::default();
        for i in 0..(MAX_SESSIONS as u64 + 10) {
            s.add("g", i * 1000, i * 1000 + 60);
        }
        assert_eq!(s.games["g"].sessions.len(), MAX_SESSIONS);
        assert_eq!(s.total_minutes("g"), MAX_SESSIONS as u64 + 10);
        for i in 0..(MAX_GAMES as u64 + 5) {
            s.add(&format!("x{i}"), 10_000_000 + i, 10_000_000 + i);
        }
        assert_eq!(s.games.len(), MAX_GAMES);
        assert!(!s.games.contains_key("g"), "the stalest game goes first");
    }

    #[test]
    fn file_round_trips_0600_and_bad_files_are_empty() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        assert_eq!(Store::load(d.path()), Store::default());
        std::fs::write(d.path().join(FILE), "{not json").unwrap();
        assert_eq!(Store::load(d.path()), Store::default());
        std::fs::write(d.path().join(FILE), r#"{"v":1,"games":{"g":"nope"}}"#).unwrap();
        assert_eq!(Store::load(d.path()), Store::default());
        let mut s = Store::default();
        s.add("g", 100, 700);
        s.save(d.path());
        assert_eq!(Store::load(d.path()), s);
        let m = std::fs::metadata(d.path().join(FILE)).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn poll_interval_defaults_and_clamps() {
        let s = |n| Duration::from_secs(n);
        assert_eq!(poll_from(None), s(15));
        assert_eq!(poll_from(Some("junk")), s(15));
        assert_eq!(poll_from(Some("1")), s(5));
        assert_eq!(poll_from(Some("20")), s(20));
    }

    // Watcher tests: a fake proc dir changes under a task running on paused time.
    use crate::game::GameId;

    fn rec(name: &str, file: &str) -> Record {
        Record {
            id: GameId::emu("dolphin", name).unwrap(),
            emulator: "dolphin".into(),
            system: None,
            file: file.into(),
            name: name.into(),
            added_at: None,
            source: "manual".into(),
            legacy_id: None,
            aliases: vec![],
        }
    }

    /// Shows `files` as running dolphin processes (pid = index + 10).
    fn set_running(proc: &Path, files: &[&str]) {
        for e in std::fs::read_dir(proc).unwrap() {
            std::fs::remove_dir_all(e.unwrap().path()).unwrap();
        }
        for (i, f) in files.iter().enumerate() {
            let p = proc.join((i + 10).to_string());
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("cmdline"), format!("dolphin-emu\0-b\0-e\0{f}\0")).unwrap();
            std::fs::write(
                p.join("stat"),
                "1 (dolphin-emu) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 5 0 0",
            )
            .unwrap();
        }
    }

    async fn pass(s: u64) {
        tokio::time::sleep(Duration::from_secs(s)).await;
    }

    fn setup(games: &[Record]) -> (tempfile::TempDir, tempfile::TempDir) {
        let (data, proc) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::write(
            data.path().join(emu::LIBRARY_FILE),
            emu::library_json(games),
        )
        .unwrap();
        (data, proc)
    }

    fn spawn(data: &Path, proc: &Path, base: u64) -> tokio::task::JoinHandle<()> {
        tokio::spawn(watch(
            data.into(),
            proc.into(),
            Duration::from_secs(15),
            base,
        ))
    }

    #[tokio::test(start_paused = true)]
    async fn start_stop_and_a_quick_restart_is_one_session() {
        let (data, proc) = setup(&[rec("a", "/a.iso")]);
        let w = spawn(data.path(), proc.path(), 1_000_000);
        pass(1).await;
        set_running(proc.path(), &["/a.iso"]);
        pass(300).await; // polls at 15..300 see it running
        set_running(proc.path(), &[]);
        pass(15).await; // poll sees it stopped
        let id = "emu:dolphin:a";
        let s = Store::load(data.path());
        let (start, end) = s.games[id].sessions[0];
        assert!(end - start >= 270 && end - start <= 300, "{start}..{end}");
        // back within the gap: same session
        set_running(proc.path(), &["/a.iso"]);
        pass(15).await;
        assert_eq!(Store::load(data.path()).games[id].sessions.len(), 1);
        // long stop, then a new session
        set_running(proc.path(), &[]);
        pass(300).await;
        set_running(proc.path(), &["/a.iso"]);
        pass(60).await;
        set_running(proc.path(), &[]);
        pass(15).await;
        assert_eq!(Store::load(data.path()).games[id].sessions.len(), 2);
        w.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn hostd_restart_mid_session_continues_without_extending_the_dead_time() {
        let (data, proc) = setup(&[rec("a", "/a.iso")]);
        set_running(proc.path(), &["/a.iso"]);
        let w = spawn(data.path(), proc.path(), 1_000_000);
        pass(125).await;
        w.abort(); // hostd dies; the game keeps running
        let before = Store::load(data.path()).games["emu:dolphin:a"]
            .sessions
            .clone();
        let died = before[0].1;
        assert!(died >= 1_000_000 + 60, "flushed while running: {before:?}");
        // hostd is back 10 minutes later; the game quit during the outage
        set_running(proc.path(), &[]);
        let w = spawn(data.path(), proc.path(), 1_000_600);
        pass(100).await;
        let after = Store::load(data.path()).games["emu:dolphin:a"]
            .sessions
            .clone();
        assert_eq!(after, before, "nothing added for the time hostd was down");
        // the game is running when this hostd finds it: a new session from now
        set_running(proc.path(), &["/a.iso"]);
        pass(61).await;
        let s = Store::load(data.path());
        assert_eq!(s.games["emu:dolphin:a"].sessions.len(), 2);
        assert_eq!(s.games["emu:dolphin:a"].sessions[0], before[0]);
        assert!(s.games["emu:dolphin:a"].sessions[1].0 >= 1_000_700);
        w.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn two_games_at_once_and_polls_never_double_count() {
        let (data, proc) = setup(&[rec("a", "/a.iso"), rec("b", "/b.iso")]);
        set_running(proc.path(), &["/a.iso", "/b.iso"]);
        let w = spawn(data.path(), proc.path(), 2_000_000);
        pass(615).await;
        let s = Store::load(data.path());
        for id in ["emu:dolphin:a", "emu:dolphin:b"] {
            assert_eq!(s.games[id].sessions.len(), 1);
            let m = s.total_minutes(id);
            assert!((9..=10).contains(&m), "{id}: {m} min");
        }
        w.abort();
    }

    #[test]
    fn tick_ignores_games_that_are_not_running() {
        let (_data, proc) = setup(&[]);
        set_running(proc.path(), &["/a.iso"]);
        let mut s = Store::default();
        let up = tick(
            &mut s,
            &[rec("a", "/a.iso"), rec("b", "/b.iso")],
            proc.path(),
            500,
        );
        assert_eq!(up, HashSet::from(["emu:dolphin:a".to_string()]));
        assert_eq!(s.games.len(), 1);
    }
}
