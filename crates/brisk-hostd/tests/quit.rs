//! POST /api/quit against a fake /proc and a fake Runner. Nothing here signals a real process:
//! `kill` and `swaymsg` only reach `Fake`, which edits the fake /proc.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{router, AppState};
use brisk_hostd::steam::{Game, GamesResult};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const SECRET: &str = "s3cret";
const SWAY_TREE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sway-tree.json");

/// Shaped on bawn's real tree (2026-10-25): Big Picture's window belongs to steamwebhelper under
/// srt-bwrap under steam, the overlay is steam's child, and each game runs under its own reaper.
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
    (501, 500, &["srt-bwrap"]),
    (502, 501, &["Mewgenics.exe"]),
    (
        600,
        400,
        &["/steam/reaper", "SteamLaunch", "AppId=20", "--", "y"],
    ),
    (601, 600, &["Other.exe"]),
];
const GAME_10: &[u32] = &[500, 501, 502];

fn fake_procs(root: &Path, procs: &[(u32, u32, &[&str])]) {
    for (pid, ppid, argv) in procs {
        let d = root.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), argv.join("\0") + "\0").unwrap();
        std::fs::write(d.join("stat"), format!("{pid} (a b) S {ppid} 1 1 0")).unwrap();
    }
}

/// What makes game 10 exit: its windows closing, SIGTERM, SIGKILL, or nothing.
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
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn exit_game_10(&self) {
        for p in GAME_10 {
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
        match p {
            "swaymsg" if a.ends_with(&["-t", "get_tree"]) => {
                return Some(std::fs::read_to_string(SWAY_TREE).unwrap())
            }
            "swaymsg" if self.exits == Exits::OnClose && a.last() == Some(&"[con_id=16] kill") => {
                self.exit_game_10()
            }
            "kill" if a[0] == "-TERM" && self.exits == Exits::OnTerm => self.exit_game_10(),
            "kill" if a[0] == "-KILL" && self.exits == Exits::OnKill => self.exit_game_10(),
            _ => {}
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

fn rig(exits: Exits) -> Rig {
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
        calls: Mutex::default(),
    });
    let mut st = AppState::with_host(cfg, fake.clone(), Arc::new(Clock0));
    st.quit.wait = Duration::from_millis(400);
    *st.games.lock().unwrap() = Some(GamesResult {
        games: vec![game(10, "Mewgenics"), game(20, "Other"), game(30, "Idle")],
        owned_available: true,
    });
    Rig {
        app: router(Arc::new(st)),
        fake,
        _d: d,
    }
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

#[tokio::test]
async fn graceful_quit_closes_the_game_windows_through_sway_only() {
    let r = rig(Exits::OnClose);
    let (st, v) = r.quit(r#"{"appid":10}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("closed")),
        "{v}"
    );
    assert_eq!(
        r.calls(),
        CLOSE,
        "only its two windows, never Big Picture (6, 7) or game 20 (26)"
    );
}

#[tokio::test]
async fn escalation_sigterms_only_the_reapers_descendants() {
    let r = rig(Exits::OnTerm);
    let (st, v) = r.quit(r#"{"appid":10}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("closed")),
        "{v}"
    );
    assert_eq!(r.calls(), [CLOSE[0], CLOSE[1], "kill -TERM 501 502"]);
    for pid in [400, 410, 411, 412, 450, 600, 601] {
        assert!(r.alive(pid), "{pid} must stay untouched");
    }
}

#[tokio::test]
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

#[tokio::test]
async fn force_sigkills_the_same_descendants_after_sigterm() {
    let r = rig(Exits::OnKill);
    let (st, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("closed")),
        "{v}"
    );
    assert_eq!(
        r.calls(),
        [
            CLOSE[0],
            CLOSE[1],
            "kill -TERM 501 502",
            "kill -KILL 501 502"
        ]
    );
    assert!(r.alive(400) && r.alive(601));
}

#[tokio::test]
async fn force_that_fails_is_still_running() {
    let r = rig(Exits::Never);
    let (_, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    assert_eq!(v["state"], "still-running");
}

#[tokio::test]
async fn known_game_that_is_not_running_is_409() {
    let r = rig(Exits::OnClose);
    let (st, _) = r.quit(r#"{"appid":30}"#).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert!(r.calls().is_empty());
}

#[tokio::test]
async fn unknown_or_bad_appid_is_400() {
    let r = rig(Exits::OnClose);
    for b in [r#"{"appid":99}"#, r#"{"appid":"10"}"#, r#"{}"#, "nope"] {
        assert_eq!(r.quit(b).await.0, StatusCode::BAD_REQUEST, "{b}");
    }
    assert!(r.calls().is_empty());
}

#[tokio::test]
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

#[tokio::test]
async fn a_second_quit_of_the_same_game_while_one_runs_is_409() {
    let r = rig(Exits::Never);
    let ((a, _), (b, _), (c, vc)) = tokio::join!(
        r.quit(r#"{"appid":10}"#),
        async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            r.quit(r#"{"appid":10}"#).await
        },
        async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            r.quit(r#"{"appid":20}"#).await // another game is not held up
        },
    );
    let mut codes = [a, b];
    codes.sort();
    assert_eq!(codes, [StatusCode::OK, StatusCode::CONFLICT]);
    assert_eq!(
        (c, vc["state"].as_str()),
        (StatusCode::OK, Some("still-running"))
    );
    // one quit of game 10 only: two windows closed once each
    let c = r.calls();
    assert_eq!(
        c.iter().filter(|c| c.ends_with("[con_id=16] kill")).count(),
        1,
        "{c:?}"
    );
}

#[tokio::test]
async fn a_client_that_hangs_up_does_not_stop_the_quit() {
    let r = rig(Exits::OnTerm);
    let call = r.app.clone().oneshot(req(r#"{"appid":10}"#, Some(SECRET)));
    assert!(tokio::time::timeout(Duration::from_millis(100), call)
        .await
        .is_err());
    // the windows were asked to close, then SIGTERM follows after `wait` with nobody listening
    for _ in 0..40 {
        if !r.alive(502) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!r.alive(502));
    assert!(r.calls().contains(&"kill -TERM 501 502".to_string()));
}
