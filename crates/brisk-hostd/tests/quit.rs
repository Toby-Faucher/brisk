//! POST /api/quit against a fake /proc, a fake Runner and a fake Signaller. Nothing here signals
//! a real process: signals and `swaymsg` only reach `Fake`, which edits the fake /proc (the real
//! pidfd Signaller is tested on a real child in running.rs). Time is paused (tokio
//! test-util), so the 10 s waits run instantly and in a fixed order.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use brisk_hostd::running::{Sent, Sig, Signaller};
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

/// Dolphin on `EMU_FILE` (800), a Dolphin on another file (801) and a non-Dolphin that has the
/// file in its argv (802): only 800 may ever be touched by quitting the emulator game.
const EMU_FILE: &str = "/games/mkw.iso";
fn emu_id() -> String {
    // What emulators.json's "Mario Kart Wii" becomes (library.json is made from it).
    "emu:dolphin:mario-kart-wii".into()
}

fn fake_emu_procs(root: &Path) {
    for (pid, comm, argv) in [
        (
            800,
            "dolphin-emu",
            vec!["dolphin-emu", "-b", "-e", EMU_FILE],
        ),
        (
            801,
            "dolphin-emu",
            vec!["dolphin-emu", "-b", "-e", "/games/other.iso"],
        ),
        (802, "vim", vec!["vim", EMU_FILE]),
    ] {
        let d = root.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), argv.join("\0") + "\0").unwrap();
        std::fs::write(d.join("stat"), stat(pid, comm, 'S', 1, 1000 + pid)).unwrap();
    }
}

fn fake_procs(root: &Path, procs: &[(u32, u32, &[&str])]) {
    for (pid, ppid, argv) in procs {
        let d = root.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), argv.join("\0") + "\0").unwrap();
        std::fs::write(d.join("stat"), stat(*pid, "a b", 'S', *ppid, 1000 + pid)).unwrap();
    }
    let z = root.join(ZOMBIE.to_string());
    std::fs::create_dir_all(&z).unwrap();
    std::fs::write(z.join("cmdline"), "").unwrap();
    std::fs::write(z.join("stat"), stat(ZOMBIE, "Chowdren", 'Z', 511, 1520)).unwrap();
}

/// A /proc/<pid>/stat line; `start` is field 22 (starttime).
fn stat(pid: u32, comm: &str, state: char, ppid: u32, start: u32) -> String {
    format!("{pid} ({comm}) {state} {ppid} 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 {start} 0 0")
}

/// What makes a game exit: its windows closing, SIGTERM, SIGKILL, or nothing.
#[derive(Clone, Copy, PartialEq)]
enum Exits {
    OnClose,
    OnTerm,
    OnKill,
    Never,
}

/// How the fake signaller answers.
#[derive(Clone, Copy, PartialEq)]
enum Fail {
    No,
    /// EPERM: nothing delivered.
    Eperm,
    /// The signal lands but the process was already exiting: ESRCH.
    Esrch,
    /// A kernel without pidfd: ENOSYS.
    Enosys,
    /// EINVAL (or EMFILE) for pid 501 only.
    Einval501,
}

struct Fake {
    proc: PathBuf,
    exits: Exits,
    fail: Fail,
    /// This pid is handed to an unrelated process between the scan and the signal.
    recycle: Option<u32>,
    /// Every swaymsg call and every signal asked for, in order.
    calls: Mutex<Vec<String>>,
    /// Signals that reached a process.
    delivered: Mutex<Vec<String>>,
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
            // Plus Dolphin's Xwayland window: no app_id, only an X11 class.
            let mut t: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(SWAY_TREE).unwrap()).unwrap();
            t["nodes"].as_array_mut().unwrap().push(serde_json::json!({
                "id": 40, "type": "con", "pid": 800, "app_id": null,
                "window_properties": {"class": "dolphin-emu"}
            }));
            return Some(t.to_string());
        }
        if p == "swaymsg" && self.exits == Exits::OnClose {
            match a.last() {
                Some(&"[con_id=16] kill") => self.gone(GAME_10),
                Some(&"[con_id=40] kill") => self.gone(&[800]),
                _ => {}
            }
        }
        Some(String::new())
    }
}

impl Signaller for Fake {
    fn signal(&self, pid: u32, sig: Sig, same: &dyn Fn() -> bool) -> Sent {
        let line = format!(
            "kill -{} {pid}",
            if sig == Sig::Term { "TERM" } else { "KILL" }
        );
        self.calls.lock().unwrap().push(line.clone());
        let d = self.proc.join(pid.to_string());
        if self.recycle == Some(pid) {
            std::fs::write(d.join("cmdline"), "innocent\0").unwrap();
            std::fs::write(d.join("stat"), stat(pid, "innocent", 'S', 1, 999_999)).unwrap();
        }
        if self.fail == Fail::Enosys {
            return Sent::Unsupported;
        }
        if self.fail == Fail::Einval501 && pid == 501 {
            return Sent::Failed("pidfd_open: Invalid argument".into());
        }
        if !d.exists() {
            return Sent::Gone;
        }
        if !same() {
            return Sent::Recycled;
        }
        if self.fail == Fail::Eperm {
            return Sent::Denied;
        }
        self.delivered.lock().unwrap().push(line);
        let hit = match sig {
            Sig::Term => self.exits == Exits::OnTerm,
            Sig::Kill => self.exits == Exits::OnKill,
        };
        if hit {
            self.gone(&[pid]);
            if pid == 502 {
                self.gone(GAME_10); // its reaper exits once the game has
            }
        }
        if self.fail == Fail::Esrch {
            return Sent::Gone;
        }
        Sent::Signalled
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
    let mut g = Game::steam(appid, name);
    g.installed = true;
    g
}

fn rig_with(exits: Exits, fail: Fail, as_root: bool) -> Rig {
    rig_full(exits, fail, None, as_root)
}

fn rig_full(exits: Exits, fail: Fail, recycle: Option<u32>, as_root: bool) -> Rig {
    let d = tempfile::tempdir().unwrap();
    let proc = d.path().join("proc");
    fake_procs(&proc, PROCS);
    fake_emu_procs(&proc);
    std::fs::create_dir_all(d.path().join("data")).unwrap();
    std::fs::write(
        d.path().join("data/emulators.json"),
        format!(r#"[{{"name":"Mario Kart Wii","emulator":"dolphin","file":"{EMU_FILE}"}}]"#),
    )
    .unwrap();
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
        fail,
        recycle,
        calls: Mutex::default(),
        delivered: Mutex::default(),
    });
    let (cfg_data, proc2) = (cfg.data_dir.clone(), cfg.proc_dir.clone());
    let mut st = AppState::with_host(cfg, fake.clone(), Arc::new(Clock0));
    st.quit.as_root = as_root;
    st.quit.signaller = fake.clone();
    let emu = brisk_hostd::emu::games(&brisk_hostd::emu::load(&cfg_data), &proc2, &cfg_data);
    *st.games.lock().unwrap() = Some(GamesResult {
        games: [
            vec![
                game(10, "Pepper Grinder"),
                game(20, "Other"),
                game(30, "Idle"),
                game(40, "Orphan"),
            ],
            emu,
        ]
        .concat(),
        owned_available: true,
    });
    Rig {
        app: router(Arc::new(st)),
        fake,
        _d: d,
    }
}

fn rig(exits: Exits) -> Rig {
    rig_with(exits, Fail::No, false)
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

/// One signal per pid, lowest first.
fn each(sig: &str) -> Vec<String> {
    KIDS_10
        .split(' ')
        .map(|p| format!("kill -{sig} {p}"))
        .collect()
}
fn term10() -> Vec<String> {
    each("TERM")
}
fn kill10() -> Vec<String> {
    each("KILL")
}
fn close() -> Vec<String> {
    CLOSE.map(String::from).to_vec()
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
    assert_eq!(r.calls(), [close(), term10()].concat());
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
    assert_eq!(r.calls(), [term10(), kill10()].concat());
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
async fn eperm_is_still_running() {
    let r = rig_with(Exits::OnTerm, Fail::Eperm, false);
    let (st, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("still-running"))
    );
    assert!(r.fake.delivered.lock().unwrap().is_empty());
    assert!(r.alive(502));
}

#[tokio::test(start_paused = true)]
async fn a_pid_vanishing_before_the_signal_is_fine() {
    // ESRCH because the processes went away under it: closed, no panic.
    let r = rig_with(Exits::OnTerm, Fail::Esrch, false);
    let (st, v) = r.quit(r#"{"appid":10}"#).await;
    assert_eq!((st, v["state"].as_str()), (StatusCode::OK, Some("closed")));
}

#[tokio::test(start_paused = true)]
async fn a_pid_recycled_between_scan_and_signal_is_not_signalled() {
    let r = rig_full(Exits::OnTerm, Fail::No, Some(502), false);
    let (st, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("still-running")),
        "{v}"
    );
    let d = r.fake.delivered.lock().unwrap().clone();
    assert!(!d.iter().any(|c| c.ends_with(" 502")), "{d:?}");
    // The other game processes were signalled; the unrelated owner of 502 lives on.
    assert!(d.contains(&"kill -TERM 501".to_string()), "{d:?}");
    assert!(r.alive(502) && r.alive(500));
}

#[tokio::test(start_paused = true)]
async fn no_pidfd_support_stops_with_an_error_and_no_fallback() {
    let r = rig_with(Exits::OnTerm, Fail::Enosys, false);
    let (_, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    assert_eq!(v["state"], "still-running");
    // TERM then KILL, each round stopping at its first pid.
    assert_eq!(r.calls(), ["kill -TERM 501", "kill -KILL 501"]);
    assert!(r.fake.delivered.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn another_error_on_one_pid_does_not_stop_the_round() {
    let r = rig_with(Exits::OnTerm, Fail::Einval501, false);
    let (_, v) = r.quit(r#"{"appid":10,"force":true}"#).await;
    assert_eq!(v["state"], "closed", "{v}");
    let d = r.fake.delivered.lock().unwrap().clone();
    assert!(!d.contains(&"kill -TERM 501".to_string()), "{d:?}");
    assert!(d.contains(&"kill -TERM 502".to_string()), "{d:?}");
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
    let r = rig_with(Exits::OnClose, Fail::No, true);
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
    assert!(
        tokio::time::timeout(Duration::from_millis(100), call)
            .await
            .is_err()
    );
    // the windows were asked to close; SIGTERM follows after 10 s with nobody listening
    for _ in 0..300 {
        if !r.alive(502) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!r.alive(502));
    assert!(r.calls().ends_with(&term10()));
}

const EMU_CLOSE: &str = "swaymsg [con_id=40] kill";

#[tokio::test(start_paused = true)]
async fn emulator_quit_closes_its_window_politely_and_touches_nothing_else() {
    let r = rig(Exits::OnClose);
    let (st, v) = r.quit(&format!(r#"{{"id":"{}"}}"#, emu_id())).await;
    assert_eq!(
        (st, v["state"].as_str()),
        (StatusCode::OK, Some("closed")),
        "{v}"
    );
    assert_eq!(r.calls(), [EMU_CLOSE]);
    assert!(!r.alive(800) && r.alive(801) && r.alive(802));
}

#[tokio::test(start_paused = true)]
async fn emulator_that_ignores_the_close_gets_sigterm_and_never_sigkill_without_force() {
    let r = rig(Exits::OnTerm);
    let t = Instant::now();
    let (_, v) = r.quit(&format!(r#"{{"id":"{}"}}"#, emu_id())).await;
    assert!(
        (Duration::from_secs(5)..Duration::from_secs(10)).contains(&t.elapsed()),
        "an emulator waits 5 s for its own stop, then SIGTERM: {:?}",
        t.elapsed()
    );
    assert_eq!(v["state"], "closed", "{v}");
    assert_eq!(r.calls(), [EMU_CLOSE, "kill -TERM 800"]);
    let r = rig(Exits::OnKill);
    let t = Instant::now();
    let (_, v) = r.quit(&format!(r#"{{"id":"{}"}}"#, emu_id())).await;
    // Close 5 s, SIGTERM 10 s, then it gives up: the UI offers Force quit.
    assert!(t.elapsed() >= Duration::from_secs(15), "{:?}", t.elapsed());
    assert_eq!(v["state"], "still-running", "{v}");
    assert_eq!(r.calls(), [EMU_CLOSE, "kill -TERM 800"]);
    assert!(r.alive(800) && r.alive(801) && r.alive(802));
}

#[tokio::test(start_paused = true)]
async fn forced_emulator_quit_skips_the_close_and_sigkills_after_sigterm() {
    let r = rig(Exits::OnKill);
    let (_, v) = r
        .quit(&format!(r#"{{"id":"{}","force":true}}"#, emu_id()))
        .await;
    assert_eq!(v["state"], "closed", "{v}");
    assert_eq!(r.calls(), ["kill -TERM 800", "kill -KILL 800"]);
    assert!(r.alive(801) && r.alive(802));
}

#[tokio::test(start_paused = true)]
async fn emulator_pid_reused_after_the_scan_is_not_signalled() {
    let r = rig_full(Exits::OnTerm, Fail::No, Some(800), false);
    let (_, v) = r
        .quit(&format!(r#"{{"id":"{}","force":true}}"#, emu_id()))
        .await;
    // The pid now belongs to an unrelated process, so the emulator counts as gone.
    assert_eq!(v["state"], "closed", "{v}");
    assert!(r.alive(800) && r.fake.delivered.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn emulator_game_that_is_not_running_is_409() {
    let r = rig(Exits::Never);
    std::fs::remove_dir_all(r.fake.proc.join("800")).unwrap();
    let (st, _) = r.quit(&format!(r#"{{"id":"{}"}}"#, emu_id())).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert!(r.calls().is_empty());
}
