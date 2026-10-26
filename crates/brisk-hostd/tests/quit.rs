//! POST /api/quit against a fake /proc and a fake Runner. Nothing here signals a real process:
//! `kill` and `swaymsg` only reach `Fake`, which edits the fake /proc. Time is paused (tokio
//! test-util), so the 10 s waits run instantly and in a fixed order.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{router, AppState};
use brisk_hostd::steam::{Game, GamesResult};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;
use tower::ServiceExt;

const SECRET: &str = "s3cret";
const SWAY_TREE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sway-tree.json");

/// Shaped on bawn's real tree (2026-10-25): Big Picture's window belongs to steamwebhelper under
/// srt-bwrap under steam, the overlay is steam's child, and each game runs under its own reaper.
/// Game 10's chain is Pepper Grinder's as `ps` shows it (reaper, srt-bwrap, pv-adverb, run.sh,
/// the game binary, which owns windows 15 and 16); the wineserver reparented to the reaper and
/// the zombie are added to cover Proton-style trees.
const PROCS: &[(u32, u32, &[&str])] = &[
    (400, 1, &["/steam/ubuntu12_32/steam", "-gamepadui"]),
    (410, 400, &["srt-bwrap", "--args", "26"]),
    (411, 410, &["./steamwebhelper", "-steampid=400"]),
    (412, 411, &["steamwebhelper", "--type=zygote"]),
    (450, 400, &["gameoverlayui", "-pid", "502", "-gameid", "10"]),
    (
        500,
        400,
        &["/steam/reaper", "SteamLaunch", "AppId=10", "--", "x"],
    ),
    (501, 500, &["srt-bwrap", "--args", "26"]),
    (510, 501, &["pv-adverb", "--generate-locales"]),
    (
        511,
        510,
        &["/bin/sh", "/steam/common/Pepper Grinder/run.sh"],
    ),
    (502, 511, &["./bin64/Chowdren"]),
    (505, 500, &["wineserver"]),
    (
        600,
        400,
        &["/steam/reaper", "SteamLaunch", "AppId=20", "--", "y"],
    ),
    (601, 600, &["Other.exe"]),
    (
        700,
        400,
        &["/steam/reaper", "SteamLaunch", "AppId=40", "--", "z"],
    ),
];
/// A zombie child of run.sh: no cmdline, state Z.
const ZOMBIE: u32 = 520;
const GAME_10: &[u32] = &[500, 501, 502, 505, 510, 511, ZOMBIE];
const KIDS_10: &str = "501 502 505 510 511 520";

fn fake_procs(root: &Path, procs: &[(u32, u32, &[&str])]) {
    for (pid, ppid, argv) in procs {
        let d = root.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), argv.join("\0") + "\0").unwrap();
        std::fs::write(d.join("stat"), format!("{pid} (a b) S {ppid} 1 1 0")).unwrap();
    }
    let z = root.join(ZOMBIE.to_string());
    std::fs::create_dir_all(&z).unwrap();
    std::fs::write(z.join("cmdline"), "").unwrap();
    std::fs::write(z.join("stat"), format!("{ZOMBIE} (Chowdren) Z 511 1 1 0")).unwrap();
}

/// What makes a game exit: its windows closing, SIGTERM, SIGKILL, or nothing.
#[derive(Clone, Copy, PartialEq)]
enum Exits {
    OnClose,
    OnTerm,
    OnKill,
    Never,
}

struct Fake {
    proc: PathBuf,
    exits: Exits,
    /// `kill` exits non-zero (a pid already gone, EPERM, no binary).
    kill_fails: bool,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn gone(&self, pids: &[u32]) {
        for p in pids {
            let _ = std::fs::remove_dir_all(self.proc.join(p.to_string()));
        }
    }
}

impl brisk_hostd::host::Runner for Fake {
    fn run(&self, p: &str, a: &[&str]) -> Option<String> {
        let line = format!("{p} {}", a.join(" "));
        // The sway socket comes from the real session; leave it out of the log.
        let line = line
            .split_once(" -s ")
            .map_or(line.clone(), |(prog, rest)| {
                format!("{prog} {}", rest.split_once(' ').map_or("", |r| r.1))
            });
        self.calls.lock().unwrap().push(line);
        if p == "swaymsg" && a.ends_with(&["-t", "get_tree"]) {
            return Some(std::fs::read_to_string(SWAY_TREE).unwrap());
        }
        if p == "swaymsg" && self.exits == Exits::OnClose && a.last() == Some(&"[con_id=16] kill") {
            self.gone(GAME_10);
        }
        if p == "kill" {
            let hit = match a[0] {
                "-TERM" => self.exits == Exits::OnTerm,
                "-KILL" => self.exits == Exits::OnKill,
                _ => false,
            };
            if hit {
                let pids: Vec<u32> = a[1..].iter().map(|p| p.parse().unwrap()).collect();
                self.gone(&pids);
                if pids.contains(&502) {
                    self.gone(GAME_10); // its reaper exits once the game has
                }
            }
            if self.kill_fails {
                return None;
            }
        }
        Some(String::new())
    }
}

struct Rig {
    app: axum::Router,
    fake: Arc<Fake>,
    _d: tempfile::TempDir,
}

struct Clock0;
impl brisk_hostd::host::Clock for Clock0 {
    fn now_s(&self) -> u64 {
        0
    }
}

fn game(appid: u32, name: &str) -> Game {
    Game {
        appid,
        name: name.into(),
        installed: true,
        size_bytes: None,
        last_played: None,
        playtime_min: 0,
        cover: String::new(),
        running: false,
    }
}

fn rig_with(exits: Exits, kill_fails: bool, as_root: bool) -> Rig {
    let d = tempfile::tempdir().unwrap();
    let proc = d.path().join("proc");
    fake_procs(&proc, PROCS);
    let mut cfg = Config::from_env(d.path().to_path_buf());
    cfg.secret = SECRET.into();
    cfg.data_dir = d.path().join("data");
    cfg.steam_dir = d.path().join("nosteam");
    cfg.steam_key_file = d.path().join("nokey");
    cfg.steam_api_base = "http://127.0.0.1:1".into();
    cfg.proc_dir = proc.clone();
    let fake = Arc::new(Fake {
        proc,
        exits,
        kill_fails,
        calls: Mutex::default(),
    });
    let mut st = AppState::with_host(cfg, fake.clone(), Arc::new(Clock0));
    st.quit.as_root = as_root;
    *st.games.lock().unwrap() = Some(GamesResult {
        games: vec![
            game(10, "Pepper Grinder"),
            game(20, "Other"),
            game(30, "Idle"),
            game(40, "Orphan"),
        ],
        owned_available: true,
    });
    Rig {
        app: router(Arc::new(st)),
        fake,
        _d: d,
    }
}

fn rig(exits: Exits) -> Rig {
    rig_with(exits, false, false)
}

fn req(body: &str, secret: Option<&str>) -> Request<Body> {
    let mut r = Request::builder()
        .method("POST")
        .uri("/api/quit")
        .header("host", "localhost:8443")
        .header("content-type", "application/json");
    if let Some(s) = secret {
        r = r.header("x-brisk-secret", s);
    }
    r.body(Body::from(body.to_string())).unwrap()
}

impl Rig {
    async fn quit(&self, body: &str) -> (StatusCode, serde_json::Value) {
        let r = self
            .app
            .clone()
            .oneshot(req(body, Some(SECRET)))
            .await
            .unwrap();
        let st = r.status();
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        (st, serde_json::from_slice(&b).unwrap_or_default())
    }
    /// Every call but the tree reads.
    fn calls(&self) -> Vec<String> {
        let c = self.fake.calls.lock().unwrap();
        c.iter()
            .filter(|c| !c.ends_with("get_tree"))
            .cloned()
            .collect()
    }
    fn alive(&self, pid: u32) -> bool {
        self.fake.proc.join(pid.to_string()).exists()
    }
}

const CLOSE: [&str; 2] = ["swaymsg [con_id=15] kill", "swaymsg [con_id=16] kill"];
const BYSTANDERS: [u32; 7] = [400, 410, 411, 412, 450, 600, 601];

fn term10() -> String {
    format!("kill -TERM {KIDS_10}")
}
fn kill10() -> String {
    format!("kill -KILL {KIDS_10}")
}

#[tokio::test(start_paused = true)]
async fn graceful_quit_closes_the_game_windows_through_sway_only() {
    let r = rig(Exits::OnClose);
    let (st, v) = r.quit(r#"{"appid":10}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("closed")),
        "{v}"
    );
    // Only its two windows: never Big Picture (6, 7), game 20 (26) or the reaper's own (9).
    assert_eq!(r.calls(), CLOSE);
    assert!(!r.calls().iter().any(|c| c.contains("con_id=9]")));
}

#[tokio::test(start_paused = true)]
async fn escalation_sigterms_only_the_reapers_descendants() {
    let r = rig(Exits::OnTerm);
    let t = Instant::now();
    let (st, v) = r.quit(r#"{"appid":10}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("closed")),
        "{v}"
    );
    assert!(
        t.elapsed() >= Duration::from_secs(10),
        "waited for the close first"
    );
    // wineserver (505, reparented to the reaper) and the zombie are the game's too; the reaper is not.
    assert_eq!(r.calls(), [CLOSE[0].into(), CLOSE[1].into(), term10()]);
    for pid in BYSTANDERS {
        assert!(r.alive(pid), "{pid} must stay untouched");
    }
}

#[tokio::test(start_paused = true)]
async fn without_force_a_stubborn_game_is_still_running_and_never_sigkilled() {
    let r = rig(Exits::OnKill);
    let (st, v) = r.quit(r#"{"appid":10}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("still-running")),
        "{v}"
    );
    assert!(
        r.calls().iter().all(|c| !c.contains("-KILL")),
        "{:?}",
        r.calls()
    );
    assert!(r.alive(502));
}

#[tokio::test(start_paused = true)]
async fn force_skips_the_close_and_sigkills_3s_after_sigterm() {
    let r = rig(Exits::OnKill);
    let t = Instant::now();
    let (st, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    let took = t.elapsed();
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("closed")),
        "{v}"
    );
    assert_eq!(r.calls(), [term10(), kill10()]);
    assert!(
        took >= Duration::from_secs(3) && took < Duration::from_secs(4),
        "{took:?}"
    );
    for pid in BYSTANDERS {
        assert!(r.alive(pid), "{pid} must stay untouched");
    }
}

#[tokio::test(start_paused = true)]
async fn force_that_fails_is_still_running() {
    let r = rig(Exits::Never);
    let (_, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    assert_eq!(v["state"], "still-running");
}

#[tokio::test(start_paused = true)]
async fn a_reaper_with_no_game_left_gets_sigterm_itself_and_only_it() {
    let r = rig(Exits::OnTerm);
    let t = Instant::now();
    let (_, v) = r.quit(r#"{"appid":40}"#).await;
    assert_eq!(v["state"], "closed", "{v}");
    assert_eq!(r.calls(), ["kill -TERM 700"]);
    assert!(
        t.elapsed() < Duration::from_secs(4),
        "no 20 s wait on an empty tree"
    );
    assert!(r.alive(400));
}

#[tokio::test(start_paused = true)]
async fn a_stuck_lone_reaper_is_sigkilled_only_with_force() {
    let r = rig(Exits::OnKill);
    let t = Instant::now();
    let (_, v) = r.quit(r#"{"appid":40}"#).await;
    assert_eq!(v["state"], "still-running");
    assert!(
        t.elapsed() < Duration::from_secs(4),
        "only a brief wait on a lone reaper"
    );
    assert_eq!(r.calls(), ["kill -TERM 700"]);
    let (_, v) = r.quit(r#"{"appid":40,"force":true}"#).await;
    assert_eq!(v["state"], "closed");
    assert_eq!(
        r.calls(),
        ["kill -TERM 700", "kill -TERM 700", "kill -KILL 700"]
    );
}

#[tokio::test(start_paused = true)]
async fn kill_failing_is_still_running() {
    let r = rig_with(Exits::Never, true, false);
    let (st, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("still-running"))
    );
}

#[tokio::test(start_paused = true)]
async fn a_pid_vanishing_before_the_signal_is_fine() {
    // kill reports failure because the processes went away under it: closed, no panic.
    let r = rig_with(Exits::OnTerm, true, false);
    let (st, v) = r.quit(r#"{"appid":10}"#).await;
    assert_eq!((st, v["state"].as_str()), (StatusCode::OK, Some("closed")));
}

#[tokio::test(start_paused = true)]
async fn known_game_that_is_not_running_is_409() {
    let r = rig(Exits::OnClose);
    let (st, v) = r.quit(r#"{"appid":30}"#).await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::CONFLICT, Some("not running"))
    );
    assert!(r.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn unknown_or_bad_appid_is_400() {
    let r = rig(Exits::OnClose);
    for b in [r#"{"appid":99}"#, r#"{"appid":"10"}"#, r#"{}"#, "nope"] {
        assert_eq!(r.quit(b).await.0, StatusCode::BAD_REQUEST, "{b}");
    }
    assert!(r.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn no_or_wrong_secret_is_403() {
    let r = rig(Exits::OnClose);
    for s in [None, Some("wrong")] {
        let res = r
            .app
            .clone()
            .oneshot(req(r#"{"appid":10}"#, s))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
    assert!(r.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn hostd_as_root_refuses_to_quit() {
    let r = rig_with(Exits::OnClose, false, true);
    let (st, _) = r.quit(r#"{"appid":10}"#).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    assert!(r.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_second_quit_of_the_same_game_while_one_runs_is_409() {
    let r = rig(Exits::Never);
    let ((a, va), (b, vb), (c, vc)) = tokio::join!(
        r.quit(r#"{"appid":10}"#),
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            r.quit(r#"{"appid":10}"#).await
        },
        async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            r.quit(r#"{"appid":20}"#).await // another game is not held up
        },
    );
    assert_eq!(
        (a, va["state"].as_str()),
        (StatusCode::OK, Some("still-running"))
    );
    assert_eq!(
        (b, vb["error"].as_str()),
        (StatusCode::CONFLICT, Some("already quitting"))
    );
    assert_eq!(
        (c, vc["state"].as_str()),
        (StatusCode::OK, Some("still-running"))
    );
    let c = r.calls();
    assert_eq!(
        c.iter().filter(|c| c.ends_with("[con_id=16] kill")).count(),
        1,
        "{c:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_client_that_hangs_up_does_not_stop_the_quit() {
    let r = rig(Exits::OnTerm);
    let call = r.app.clone().oneshot(req(r#"{"appid":10}"#, Some(SECRET)));
    assert!(tokio::time::timeout(Duration::from_millis(100), call)
        .await
        .is_err());
    // the windows were asked to close; SIGTERM follows after 10 s with nobody listening
    for _ in 0..300 {
        if !r.alive(502) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!r.alive(502));
    assert!(r.calls().contains(&term10()));
}
