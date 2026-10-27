use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use std::sync::Arc;
use tower::ServiceExt;

fn app(dist: &std::path::Path) -> axum::Router {
    let mut cfg = Config::from_env(dist.to_path_buf());
    cfg.secret = SECRET.into();
    router(Arc::new(AppState::new(cfg)))
}

async fn send(
    dist: &std::path::Path,
    method: &str,
    uri: &str,
    host: &str,
    secret: Option<&str>,
) -> Response {
    let mut r = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", host);
    if let Some(s) = secret {
        r = r.header("x-brisk-secret", s);
    }
    app(dist)
        .oneshot(r.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body(r: Response) -> String {
    let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    String::from_utf8(b.to_vec()).unwrap()
}

fn dist() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("index.html"), "<h1>home</h1>").unwrap();
    std::fs::create_dir(d.path().join("library")).unwrap();
    std::fs::write(d.path().join("library/index.html"), "<h1>lib</h1>").unwrap();
    d
}

#[tokio::test]
async fn serves_index_from_dist() {
    let d = dist();
    let r = send(d.path(), "GET", "/", H, None).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(body(r).await, "<h1>home</h1>");
    assert_eq!(
        body(send(d.path(), "GET", "/library", "127.0.0.1:8443", None).await).await,
        "<h1>lib</h1>"
    );
    // unknown non-API path falls back to the index; unknown API path is a 404
    assert_eq!(
        body(send(d.path(), "GET", "/nope", H, None).await).await,
        "<h1>home</h1>"
    );
    assert_eq!(
        send(d.path(), "GET", "/api/nope", H, Some(SECRET))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    // no escaping dist
    assert_ne!(
        send(d.path(), "GET", "/../../etc/passwd", H, None)
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn foreign_host_is_403() {
    let d = dist();
    for h in ["evil.test", "localhost:9999", "localhost"] {
        assert_eq!(
            send(d.path(), "GET", "/", h, Some(SECRET)).await.status(),
            StatusCode::FORBIDDEN,
            "{h}"
        );
    }
}

#[tokio::test]
async fn post_without_secret_is_403() {
    let d = dist();
    for m in ["POST", "DELETE"] {
        assert_eq!(
            send(d.path(), m, "/api/stream", H, None).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(d.path(), m, "/api/stream", H, Some("wrong"))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn post_with_secret_passes_auth() {
    let d = dist();
    assert_eq!(
        send(d.path(), "POST", "/api/unknown", H, Some(SECRET))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn get_needs_secret_on_api_only() {
    let d = dist();
    assert_eq!(
        send(d.path(), "GET", "/api/unknown", H, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(d.path(), "GET", "/api/unknown", H, Some(SECRET))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(d.path(), "GET", "/", H, None).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn get_stream_needs_secret() {
    let r = rig("");
    r.call("POST", "/api/stream", r#"{"preset":"balanced"}"#)
        .await;
    let no = Request::builder()
        .uri("/api/stream")
        .header("host", H)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        r.app.clone().oneshot(no).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(r.call("GET", "/api/stream", "").await.0, StatusCode::OK);
    r.call("DELETE", "/api/stream", "").await;
}

#[tokio::test]
async fn start_after_shutdown_is_refused_and_spawns_nothing() {
    let r = rig("");
    r.state.stream.shutdown().await;
    let (st, _) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced"}"#)
        .await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    assert!(r.log().is_empty(), "no bridge may be spawned");
}

#[tokio::test]
async fn missing_asset_is_404_but_unknown_page_falls_back() {
    let d = dist();
    for p in ["/_astro/gone.js", "/a/b.css", "/favicon.ico"] {
        assert_eq!(
            send(d.path(), "GET", p, H, None).await.status(),
            StatusCode::NOT_FOUND,
            "{p}"
        );
    }
    assert_eq!(
        body(send(d.path(), "GET", "/stream/deep", H, None).await).await,
        "<h1>home</h1>"
    );
}

// ---- Steam library ----

use std::path::{Path, PathBuf};

/// Mock whose GetOwnedGames answers `status` (200 = the fixture, only for the exact query).
async fn owned_mock(status: u16) -> String {
    let body = if status == 200 {
        std::fs::read_to_string(format!("{FIX}/owned.json")).unwrap()
    } else {
        String::new()
    };
    mock_steam(vec![(
        "/IPlayerService/GetOwnedGames/v1/?steamid=76561198000000000&include_appinfo=1&include_played_free_games=1",
        status,
        body,
    )])
    .await
}

mod common;
use common::*;

fn find(v: &serde_json::Value, id: u64) -> &serde_json::Value {
    v["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["appid"] == id)
        .unwrap()
}

#[test]
fn acf_parser_handles_odd_files() {
    use brisk_hostd::steam::parse_acf;
    assert!(parse_acf("").is_none());
    assert!(parse_acf("\"AppState\" { \"appid\" ").is_none());
    assert!(parse_acf("\"AppState\"\n{\n\"appid\" \"x\"\n\"name\" \"n\"\n}").is_none());
    let i = parse_acf("\"AppState\"{\"appid\"\"5\" \"name\"\"a \\\"q\\\" é\" \"StateFlags\"\"4\" \"SizeOnDisk\"\"9\" \"LastPlayed\"\"7\"}").unwrap();
    assert_eq!(
        (
            i.appid,
            i.name.as_str(),
            i.size,
            i.state_flags,
            i.last_played
        ),
        (5, "a \"q\" é", 9, 4, 7)
    );
}

#[tokio::test]
async fn installed_games_parsed_from_all_libraries() {
    let s = steam("http://127.0.0.1:1", "http://127.0.0.1:1", None);
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    let g = find(&v, 1091500);
    assert_eq!(g["name"], "Pokémon \"Quoted\" Legends");
    assert_eq!(g["installed"], true);
    assert_eq!(g["sizeBytes"], 3000000000u64);
    assert_eq!(g["lastPlayed"], 1710000000u64);
    assert_eq!(
        g["cover"],
        "https://cdn.cloudflare.steamstatic.com/steam/apps/1091500/library_600x900.jpg"
    );
    assert_eq!(find(&v, 620)["installed"], true);
}

#[tokio::test]
async fn partial_install_is_not_installed() {
    let s = steam("http://127.0.0.1:1", "http://127.0.0.1:1", None);
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    let g = find(&v, 730);
    assert_eq!(g["installed"], false);
    assert!(g["sizeBytes"].is_null());
}

#[tokio::test]
async fn games_have_art_urls() {
    let s = steam("http://127.0.0.1:1", "http://127.0.0.1:1", None);
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    let g = find(&v, 620);
    assert_eq!(
        g["hero"],
        "https://cdn.cloudflare.steamstatic.com/steam/apps/620/library_hero.jpg"
    );
    assert_eq!(
        g["logo"],
        "https://cdn.cloudflare.steamstatic.com/steam/apps/620/logo.png"
    );
}

#[tokio::test]
async fn games_have_fortnight_minutes() {
    let s = steam(&owned_mock(200).await, "http://127.0.0.1:1", Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(find(&v, 620)["playtime2wkMin"], 192u64);
    assert_eq!(find(&v, 220)["playtime2wkMin"], 0u64);
}

#[tokio::test]
async fn owned_games_merged_with_installed() {
    let s = steam(&owned_mock(200).await, "http://127.0.0.1:1", Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(v["ownedAvailable"], true);
    assert_eq!(v["games"].as_array().unwrap().len(), 6);
    let p = find(&v, 620);
    assert_eq!(
        (p["installed"].as_bool(), p["playtimeMin"].as_u64()),
        (Some(true), Some(600))
    );
    assert_eq!(p["lastPlayed"], 1700000500u64); // owned list is newer than the manifest
    assert_eq!(find(&v, 220)["installed"], false);
    assert!(find(&v, 220)["lastPlayed"].is_null());
    // lastPlayed desc, then name: Pokemon (1710000000) first, never-played last-ish by name
    let ids: Vec<u64> = v["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["appid"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [1091500, 620, 730, 400, 570, 220]);
    assert!(s.cfg.data_dir.join("owned.json").is_file());
}

#[tokio::test]
async fn duplicate_appid_listed_once() {
    let s = steam("http://127.0.0.1:1", "http://127.0.0.1:1", None);
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(
        v["games"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|g| g["appid"] == 620)
            .count(),
        1
    );
    assert_eq!(find(&v, 620)["name"], "Portal 2");
}

#[tokio::test]
async fn api_down_still_lists_installed() {
    let s = steam(&owned_mock(500).await, "http://127.0.0.1:1", Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(v["ownedAvailable"], false);
    assert_eq!(v["games"].as_array().unwrap().len(), 3);
    assert_eq!(find(&v, 620)["installed"], true);
}

#[tokio::test]
async fn missing_key_file_is_not_an_error() {
    let s = steam(&owned_mock(200).await, "http://127.0.0.1:1", None);
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(v["ownedAvailable"], false);
    assert_eq!(v["games"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn owned_cache_is_used_until_refresh() {
    let s = steam(&owned_mock(200).await, "http://127.0.0.1:1", Some(KEY));
    games_json(&s.cfg, "/api/games").await;
    let mut dead = s.cfg.clone();
    dead.steam_api_base = "http://127.0.0.1:1".into();
    assert_eq!(
        games_json(&dead, "/api/games").await.0["ownedAvailable"],
        true
    );
    assert_eq!(
        games_json(&dead, "/api/games?refresh=1").await.0["ownedAvailable"],
        false
    );
}

#[tokio::test]
async fn is_known_appid_uses_cached_library() {
    let s = steam(&owned_mock(200).await, "http://127.0.0.1:1", Some(KEY));
    let st = AppState::new(s.cfg.clone());
    assert!(st.is_known_appid(570).await);
    assert!(!st.is_known_appid(999).await);
}

#[tokio::test]
async fn key_never_in_response() {
    let mut out = String::new();
    for status in [200, 500] {
        let s = steam(&owned_mock(status).await, "http://127.0.0.1:1", Some(KEY));
        out += &games_json(&s.cfg, "/api/games").await.1;
        out += &format!("{:?}", s.cfg);
    }
    assert!(!out.is_empty() && !out.contains(KEY) && !out.contains("0123456789abcdef"));
}

#[tokio::test]
async fn sunshine_restart_204_then_429_and_host_is_json() {
    use brisk_hostd::host::{Clock, Runner};
    struct R(std::sync::Mutex<u32>);
    impl Runner for R {
        fn run(&self, _: &str, _: &[&str]) -> Option<String> {
            *self.0.lock().unwrap() += 1;
            Some(String::new())
        }
    }
    struct C;
    impl Clock for C {
        fn now_s(&self) -> u64 {
            1000
        }
    }
    let d = dist();
    let mut cfg = Config::from_env(d.path().to_path_buf());
    cfg.secret = SECRET.into();
    let runner = Arc::new(R(Default::default()));
    let app = router(Arc::new(AppState::with_host(
        cfg,
        runner.clone(),
        Arc::new(C),
    )));
    let post = || {
        Request::builder()
            .method("POST")
            .uri("/api/sunshine/restart")
            .header("host", H)
            .header("x-brisk-secret", SECRET)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(post()).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    let r = app.clone().oneshot(post()).await.unwrap();
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    let wait: u64 = r.headers()["retry-after"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=100).contains(&wait), "Retry-After {wait}");
    assert!(body(r).await.contains("error"));
    assert_eq!(*runner.0.lock().unwrap(), 1);
    let r = app
        .oneshot(
            Request::builder()
                .uri("/api/host")
                .header("host", H)
                .header("x-brisk-secret", SECRET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body(r).await).unwrap();
    assert!(v["sunshine"].is_string() && v.get("uptimeS").is_some());
}

// ---- Task 4: stream supervisor, launch, install, sessions. Never real steam or bridge. ----

const SWAY_TREE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sway-tree.json");

/// A fake `/proc`: `(pid, ppid, argv)` per process, as `<pid>/stat` and NUL-separated `<pid>/cmdline`.
fn fake_procs(root: &Path, procs: &[(u32, u32, &[&str])]) {
    for (pid, ppid, argv) in procs {
        let d = root.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), argv.join("\0") + "\0").unwrap();
        // comm with a space and a paren, as real ones can have
        std::fs::write(
            d.join("stat"),
            format!("{pid} (a b) c) S {ppid} 1 1 0 -1 0"),
        )
        .unwrap();
    }
}

/// Steam (400) launched game 10 through its reaper (500); 502 owns the game window in sway-tree.json.
fn game_10_running(root: &Path) {
    fake_procs(
        root,
        &[
            (400, 1, &["steam"]),
            (
                500,
                400,
                &["/steam/reaper", "SteamLaunch", "AppId=10", "--", "proton"],
            ),
            (501, 500, &["srt-bwrap"]),
            (502, 501, &["Mewgenics.exe"]),
        ],
    );
}

const FAKE_BRIDGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake-bridge.sh");

/// `run` is slow on purpose: steam commands must use the detached `spawn`, never wait.
struct Calls(std::sync::Mutex<Vec<String>>);
impl brisk_hostd::host::Runner for Calls {
    fn run(&self, p: &str, a: &[&str]) -> Option<String> {
        if p == "swaymsg" {
            if a.ends_with(&["-t", "get_tree"]) {
                return Some(std::fs::read_to_string(SWAY_TREE).unwrap());
            }
        } else {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        self.0
            .lock()
            .unwrap()
            .push(format!("run {p} {}", a.join(" ")));
        Some(String::new())
    }
    fn spawn(&self, p: &str, a: &[&str], _env: &[(String, String)]) -> bool {
        self.0
            .lock()
            .unwrap()
            .push(format!("spawn {p} {}", a.join(" ")));
        true
    }
}
struct Clock0;
impl brisk_hostd::host::Clock for Clock0 {
    fn now_s(&self) -> u64 {
        0
    }
}

struct Rig {
    proc: PathBuf,
    app: axum::Router,
    state: Arc<AppState>,
    runner: Arc<Calls>,
    log: PathBuf,
    data: PathBuf,
    _d: tempfile::TempDir,
}

fn rig(mode: &str) -> Rig {
    rig_with(mode, false)
}

/// `service`: hostd as under systemd (stdout = journal).
fn rig_with(mode: &str, service: bool) -> Rig {
    use brisk_hostd::steam::{Game, GamesResult};
    use std::os::unix::fs::PermissionsExt;
    let d = dist();
    let log = d.path().join("bridge.log");
    let bin = d.path().join("bridge.sh");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\nFAKE_BRIDGE_LOG='{}' FAKE_BRIDGE_MODE='{mode}' exec '{FAKE_BRIDGE}' \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cfg = Config::from_env(d.path().to_path_buf());
    cfg.secret = SECRET.into();
    cfg.bridge_bin = bin;
    cfg.bridge_listen = Some("100.64.0.1:4433".into());
    // A fresh pair, so starts never run openssl (and never look at the real ~/.config/brisk).
    cfg.tls_dir = d.path().join(".config/brisk/tls");
    std::fs::create_dir_all(&cfg.tls_dir).unwrap();
    for f in ["bridge.key.pem", "bridge.pem"] {
        // key first: a key newer than its cert would count as a mismatched pair
        std::fs::write(cfg.tls_dir.join(f), "-----BEGIN fake\n").unwrap();
    }
    cfg.app_id = 42;
    cfg.service = service;
    cfg.data_dir = d.path().join("data");
    cfg.steam_dir = d.path().join("nosteam");
    cfg.steam_key_file = d.path().join("nokey");
    cfg.steam_api_base = "http://127.0.0.1:1".into();
    cfg.proc_dir = d.path().join("proc");
    let data = cfg.data_dir.clone();
    let runner = Arc::new(Calls(Default::default()));
    let st = AppState::with_host(cfg, runner.clone(), Arc::new(Clock0));
    *st.games.lock().unwrap() = Some(GamesResult {
        games: vec![Game {
            appid: 10,
            name: "Half-Life".into(),
            installed: true,
            size_bytes: None,
            last_played: None,
            playtime_min: 0,
            playtime2wk_min: 0,
            cover: String::new(),
            hero: String::new(),
            logo: String::new(),
            running: false,
            ach_unlocked: None,
            ach_total: None,
            updated: false,
        }],
        owned_available: true,
    });
    let state = Arc::new(st);
    Rig {
        proc: d.path().join("proc"),
        app: router(state.clone()),
        state,
        runner,
        log,
        data,
        _d: d,
    }
}

impl Rig {
    async fn call(&self, method: &str, uri: &str, body_s: &str) -> (StatusCode, serde_json::Value) {
        let r = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", H)
            .header("x-brisk-secret", SECRET)
            .header("content-type", "application/json")
            .body(Body::from(body_s.to_string()))
            .unwrap();
        let r = self.app.clone().oneshot(r).await.unwrap();
        let st = r.status();
        let b = body(r).await;
        (
            st,
            serde_json::from_str(&b).unwrap_or(serde_json::Value::Null),
        )
    }
    fn log(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
    fn sway_cmds(&self) -> Vec<String> {
        self.runner
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("run swaymsg"))
            .cloned()
            .collect()
    }
    fn steam_calls(&self) -> Vec<String> {
        self.runner
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.contains("steam://"))
            .cloned()
            .collect()
    }
}

async fn eventually(f: impl Fn() -> bool) -> bool {
    for _ in 0..100 {
        if f() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

/// Never two bridges alive: every start after the first follows a term of the previous one.
fn assert_one_at_a_time(log: &[String]) {
    let mut alive: Option<String> = None;
    for l in log {
        let mut w = l.split(' ');
        match (w.next(), w.next()) {
            (Some("start"), Some(pid)) => {
                assert!(alive.is_none(), "two bridges alive: {log:?}");
                alive = Some(pid.into());
            }
            (Some("term"), Some(pid)) => {
                assert_eq!(alive.as_deref(), Some(pid), "{log:?}");
                alive = None;
            }
            _ => panic!("odd log line {l}"),
        }
    }
}

#[tokio::test]
async fn start_returns_wt_and_hash() {
    let r = rig("");
    let (st, v) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced"}"#)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "live");
    assert_eq!(v["preset"], "balanced");
    assert_eq!(v["appid"], serde_json::Value::Null);
    assert_eq!(v["hash"], "ab12cd");
    let wt = v["wt"].as_str().unwrap();
    assert!(wt.starts_with("https://100.64.0.1:4433/tok"), "{wt}");
    let (mut g, mut v) = (r.call("GET", "/api/stream", "").await.1, v);
    // elapsedS moves on between the two answers
    assert!(g["elapsedS"].take().as_f64() >= v["elapsedS"].take().as_f64());
    assert_eq!(g, v);
    let log = r.log();
    assert!(
        log[0].contains("--host 127.0.0.1 --app 42 --listen 100.64.0.1:4433 --cert ")
            && log[0].contains("/.config/brisk/tls/bridge.pem --key ")
            && log[0].contains(
                "/.config/brisk/tls/bridge.key.pem --codec h264 --page http://localhost:8443/"
            ),
        "{log:?}"
    );
    r.call("DELETE", "/api/stream", "").await;
}

/// Service mode hides the bridge's page line from stdout but still parses it for the API.
#[tokio::test]
async fn service_mode_still_returns_wt_and_hash() {
    let r = rig_with("", true);
    assert!(r.state.stream.quiet);
    let (st, v) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced"}"#)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        v["wt"]
            .as_str()
            .unwrap()
            .starts_with("https://100.64.0.1:4433/tok")
    );
    assert_eq!(v["hash"], "ab12cd");
    r.call("DELETE", "/api/stream", "").await;
}

#[tokio::test]
async fn preset_maps_to_flags() {
    let r = rig("");
    for (req, flags) in [
        (
            r#"{"preset":"fast"}"#,
            "--width 1920 --height 1080 --fps 60 --bitrate-kbps 10000",
        ),
        (
            r#"{"preset":"balanced"}"#,
            "--width 1920 --height 1080 --fps 60 --bitrate-kbps 20000",
        ),
        (
            r#"{"preset":"crisp"}"#,
            "--width 1920 --height 1080 --fps 60 --bitrate-kbps 40000",
        ),
        (
            r#"{"preset":"custom","width":1280,"height":720,"fps":120,"bitrateKbps":15000}"#,
            "--width 1280 --height 720 --fps 120 --bitrate-kbps 15000",
        ),
    ] {
        let (st, v) = r.call("POST", "/api/stream", req).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let log = r.log();
        let last = log.iter().rev().find(|l| l.starts_with("start")).unwrap();
        assert!(last.ends_with(flags), "{req}: {last}");
    }
    let n = r.log().len();
    for bad in [
        r#"{"preset":"custom","width":1280,"height":720,"fps":500,"bitrateKbps":15000}"#,
        r#"{"preset":"custom","width":100,"height":720,"fps":60,"bitrateKbps":15000}"#,
        r#"{"preset":"turbo"}"#,
        "not json",
    ] {
        assert_eq!(
            r.call("POST", "/api/stream", bad).await.0,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    assert_eq!(r.log().len(), n, "a bad request must not touch the bridge");
    r.call("DELETE", "/api/stream", "").await;
}

#[tokio::test]
async fn codec_is_passed_to_the_bridge() {
    let r = rig("");
    for (req, codec) in [
        (r#"{"preset":"balanced"}"#, "h264"),
        (r#"{"preset":"crisp","codec":"hevc"}"#, "hevc"),
        (
            r#"{"preset":"custom","width":1280,"height":720,"codec":"h264"}"#,
            "h264",
        ),
    ] {
        let (st, v) = r.call("POST", "/api/stream", req).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!(v["codec"], codec, "{req}");
        assert_eq!(r.call("GET", "/api/stream", "").await.1["codec"], codec);
        let log = r.log();
        let last = log.iter().rev().find(|l| l.starts_with("start")).unwrap();
        assert!(
            last.contains(&format!(" --codec {codec} ")),
            "{req}: {last}"
        );
    }
    let n = r.log().len();
    for bad in [
        r#"{"preset":"balanced","codec":"av1"}"#,
        r#"{"preset":"balanced","codec":1}"#,
    ] {
        let (st, v) = r.call("POST", "/api/stream", bad).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}: {v}");
    }
    assert_eq!(r.log().len(), n, "a bad codec must not touch the bridge");
    r.call("DELETE", "/api/stream", "").await;
    assert_eq!(
        r.call("GET", "/api/stream", "").await.1["codec"],
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn second_start_stops_first() {
    let r = rig("");
    let (_, a) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced"}"#)
        .await;
    let (st, b) = r.call("POST", "/api/stream", r#"{"preset":"crisp"}"#).await;
    assert_eq!(st, StatusCode::OK);
    assert_ne!(a["wt"], b["wt"]);
    let log = r.log();
    assert_eq!(log.len(), 3, "{log:?}");
    let pid_a = log[0].split(' ').nth(1).unwrap();
    assert_eq!(log[1], format!("term {pid_a}"));
    assert!(log[2].starts_with("start ") && log[2].ends_with("40000"));
    assert_one_at_a_time(&log);
    r.call("DELETE", "/api/stream", "").await;
}

#[tokio::test]
async fn concurrent_starts_are_serialized() {
    let r = rig("");
    let (a, b, c) = tokio::join!(
        r.call("POST", "/api/stream", r#"{"preset":"fast"}"#),
        r.call("POST", "/api/stream", r#"{"preset":"crisp"}"#),
        r.call("POST", "/api/stream", r#"{"preset":"balanced"}"#),
    );
    for x in [&a, &b, &c] {
        assert_eq!(x.0, StatusCode::OK, "{}", x.1);
    }
    let log = r.log();
    assert_eq!(log.iter().filter(|l| l.starts_with("start")).count(), 3);
    assert_one_at_a_time(&log);
    // The bridge that is still running is the one GET reports.
    let live = r.call("GET", "/api/stream", "").await.1;
    let last_pid = log.last().unwrap().split(' ').nth(1).unwrap().to_string();
    assert!(
        live["wt"]
            .as_str()
            .unwrap()
            .ends_with(&format!("tok{last_pid}")),
        "{live} {log:?}"
    );
    r.call("DELETE", "/api/stream", "").await;
}

#[tokio::test]
async fn bridge_exit_before_page_line_is_an_error_and_idle() {
    let r = rig("early_exit");
    let (st, v) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced"}"#)
        .await;
    assert!(st.is_server_error(), "{st}");
    assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{v}");
    assert_eq!(r.call("GET", "/api/stream", "").await.1["state"], "idle");
    assert!(
        r.call("GET", "/api/sessions", "").await.1["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn appid_launches_steam_after_tap_only_on_fresh_start() {
    let r = rig("");
    let (st, v) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced","appid":10}"#)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["appid"], 10);
    assert!(r.steam_calls().is_empty(), "steam ran before the tap line");
    assert!(eventually(|| r.steam_calls().len() == 1).await);
    assert!(r.steam_calls()[0].ends_with("steam steam://rungameid/10"));
    // Quality change: no appid, so no second launch; the stream still belongs to the game.
    let (_, v) = r.call("POST", "/api/stream", r#"{"preset":"fast"}"#).await;
    assert_eq!(v["appid"], 10);
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(r.steam_calls().len(), 1);
    r.call("DELETE", "/api/stream", "").await;
    let s = r.call("GET", "/api/sessions", "").await.1;
    assert_eq!(
        s["sessions"].as_array().unwrap().len(),
        1,
        "quality change is one session: {s}"
    );
}

#[tokio::test]
async fn unknown_appid_is_400() {
    let r = rig("");
    for (uri, b) in [
        ("/api/stream", r#"{"preset":"balanced","appid":999}"#),
        ("/api/launch", r#"{"appid":999}"#),
        ("/api/install", r#"{"appid":999}"#),
    ] {
        let (st, v) = r.call("POST", uri, b).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{uri}");
        assert!(v["error"].is_string());
    }
    assert!(r.log().is_empty());
    assert!(r.runner.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn non_numeric_appid_is_400() {
    let r = rig("");
    for id in [
        r#""10""#,
        r#""10; rm -rf /""#,
        "-1",
        "1.5",
        "null",
        "{}",
        "4294967296",
    ] {
        for uri in ["/api/launch", "/api/install"] {
            let (st, _) = r.call("POST", uri, &format!(r#"{{"appid":{id}}}"#)).await;
            assert_eq!(st, StatusCode::BAD_REQUEST, "{uri} {id}");
        }
        if id != "null" {
            let b = format!(r#"{{"preset":"balanced","appid":{id}}}"#);
            assert_eq!(
                r.call("POST", "/api/stream", &b).await.0,
                StatusCode::BAD_REQUEST,
                "{id}"
            );
        }
    }
    assert_eq!(
        r.call("POST", "/api/launch", "").await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(r.log().is_empty());
    assert!(r.runner.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn launch_and_install_run_steam() {
    let r = rig("");
    let t = std::time::Instant::now();
    assert_eq!(
        r.call("POST", "/api/launch", r#"{"appid":10}"#).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        r.call("POST", "/api/install", r#"{"appid":10}"#).await.0,
        StatusCode::NO_CONTENT
    );
    let c = r.steam_calls();
    assert!(
        t.elapsed() < std::time::Duration::from_secs(1),
        "waited for steam"
    );
    assert_eq!(
        c,
        [
            "spawn steam steam://rungameid/10",
            "spawn steam steam://install/10"
        ]
    );
    assert!(r.log().is_empty(), "launch alone does not start a bridge");
}

#[tokio::test]
async fn delete_stops_and_records_session() {
    let r = rig("");
    r.call("POST", "/api/stream", r#"{"preset":"crisp","appid":10}"#)
        .await;
    let (st, v) = r.call("DELETE", "/api/stream", "").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["state"], "idle");
    assert_eq!(v["wt"], serde_json::Value::Null);
    let log = r.log();
    assert!(log[1].starts_with("term "), "{log:?}");
    let s = r.call("GET", "/api/sessions", "").await.1;
    let s0 = &s["sessions"][0];
    assert_eq!(s0["appid"], 10);
    assert_eq!(s0["name"], "Half-Life");
    assert_eq!(s0["preset"], "crisp");
    assert!(s0["host"].is_string());
    assert!(s0["ended"].as_u64().unwrap() >= s0["started"].as_u64().unwrap());
    // The fake bridge prints the real report on SIGTERM; hostd keeps it in the record.
    assert_eq!(s0["codec"], "h264");
    assert_eq!(s0["bitrateKbps"], 40000);
    assert_eq!(s0["stats"]["frames"], 58193);
    assert_eq!(s0["stats"]["bridgeP50"], 4.3);
    assert_eq!(s0["stats"]["gate"], "pass");
    // DELETE with nothing running is fine
    assert_eq!(r.call("DELETE", "/api/stream", "").await.0, StatusCode::OK);
}

#[tokio::test]
async fn bridge_exiting_by_itself_goes_idle_and_closes_session() {
    let r = rig("quit");
    let (st, _) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced"}"#)
        .await;
    assert_eq!(st, StatusCode::OK);
    let mut idle = false;
    for _ in 0..100 {
        if r.call("GET", "/api/stream", "").await.1["state"] == "idle" {
            idle = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(idle);
    let s = r.call("GET", "/api/sessions", "").await.1;
    assert_eq!(s["sessions"][0]["name"], "Desktop");
    assert!(s["sessions"][0]["ended"].is_u64(), "{s}");
    assert_eq!(s["sessions"][0]["stats"]["durationS"], 970.2, "{s}");
}

/// The page PUTs its series to `started`; a quality change keeps the session and adds up both reports.
#[tokio::test]
async fn live_info_names_the_session_and_quality_change_merges_reports() {
    let r = rig("");
    let (_, v) = r
        .call("POST", "/api/stream", r#"{"preset":"fast","appid":10}"#)
        .await;
    let started = v["started"].as_u64().expect("started while live");
    let e0 = v["elapsedS"].as_f64().expect("elapsedS while live");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let g = r.call("GET", "/api/stream", "").await.1;
    assert_eq!(g["started"], started);
    // hostd's monotonic seconds since the session started: the page's series time base
    let e1 = g["elapsedS"].as_f64().unwrap();
    assert!(e0 >= 0.0 && e1 >= e0 + 0.15 && e1 < 30.0, "{e0} {e1}");
    let (_, v) = r
        .call(
            "POST",
            "/api/stream",
            r#"{"preset":"crisp","codec":"hevc"}"#,
        )
        .await;
    assert_eq!(v["started"], started, "same session after a quality change");
    assert!(
        v["elapsedS"].as_f64().unwrap() >= e1,
        "the clock carries on: {v}"
    );
    let (_, v) = r.call("DELETE", "/api/stream", "").await;
    assert_eq!(v["started"], serde_json::Value::Null);
    assert_eq!(v["elapsedS"], serde_json::Value::Null);
    let s = r.call("GET", "/api/sessions", "").await.1;
    let s0 = &s["sessions"][0];
    assert_eq!(s["sessions"].as_array().unwrap().len(), 1, "{s}");
    assert_eq!(s0["started"], started);
    assert_eq!(
        (&s0["preset"], &s0["codec"]),
        (&"crisp".into(), &"hevc".into())
    );
    assert_eq!(s0["stats"]["frames"], 2 * 58193, "{s}");
    assert_eq!(s0["stats"]["durationS"], 1940.4);
}

#[tokio::test]
async fn sessions_returns_latest_first() {
    let r = rig("");
    std::fs::create_dir_all(&r.data).unwrap();
    let mut lines = String::new();
    for i in 1..=125u64 {
        let ended = if i == 125 {
            "null".to_string()
        } else {
            (i * 1000 + 5).to_string()
        };
        lines += &format!(
            r#"{{"appid":{i},"name":"g{i}","started":{},"ended":{ended},"host":"bawn","preset":"fast"}}"#,
            i * 1000
        );
        lines += "\n";
    }
    lines += "garbage line\n";
    // the open record of 125 is closed by a later line with the same start
    lines += r#"{"appid":125,"name":"g125","started":125000,"ended":125009,"host":"bawn","preset":"fast"}"#;
    lines += "\n";
    std::fs::write(r.data.join("sessions.jsonl"), lines).unwrap();
    let s = r.call("GET", "/api/sessions", "").await.1;
    let s = s["sessions"].as_array().unwrap();
    assert_eq!(s.len(), 100);
    assert_eq!(s[0]["started"], 125000);
    assert_eq!(s[0]["ended"], 125009);
    assert_eq!(s[1]["started"], 124000);
    assert_eq!(s[99]["started"], 26000);
    // old records (no codec, bitrate or stats) still read, as nulls
    assert_eq!(s[1]["stats"], serde_json::Value::Null);
    assert_eq!(s[1]["codec"], serde_json::Value::Null);
    assert!(s.iter().all(|r| r.get("series").is_none()));
}

#[tokio::test]
async fn dropped_request_mid_stop_still_leaves_one_bridge() {
    let r = rig("slow_term");
    let (st, _) = r.call("POST", "/api/stream", r#"{"preset":"fast"}"#).await;
    assert_eq!(st, StatusCode::OK);
    // The client goes away while hostd waits for the first bridge to exit.
    let quit = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        r.call("POST", "/api/stream", r#"{"preset":"crisp"}"#),
    )
    .await;
    assert!(
        quit.is_err(),
        "expected the request to still be stopping the first bridge"
    );
    let (st, v) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced"}"#)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    r.call("DELETE", "/api/stream", "").await;
    let log = r.log();
    assert_eq!(
        log.iter().filter(|l| l.starts_with("start")).count(),
        3,
        "{log:?}"
    );
    assert_eq!(
        log.iter().filter(|l| l.starts_with("term")).count(),
        3,
        "{log:?}"
    );
    assert_one_at_a_time(&log);
    assert_eq!(r.call("GET", "/api/stream", "").await.1["state"], "idle");
}

#[tokio::test]
async fn steam_tools_are_not_games() {
    // With the owned list: installed apps missing from it are dropped.
    let s = steam(&owned_mock(200).await, "http://127.0.0.1:1", Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert!(
        v["games"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["appid"] != 1628350 && g["appid"] != 1493710)
    );
    // Without it: dropped by name prefix, real games stay.
    let s = steam(&owned_mock(500).await, "http://127.0.0.1:1", Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(v["ownedAvailable"], false);
    assert_eq!(v["games"].as_array().unwrap().len(), 3);
    assert!(
        v["games"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["appid"] != 1628350 && g["appid"] != 1493710)
    );
}

#[tokio::test]
async fn running_game_streams_without_relaunch_and_fullscreens_its_window() {
    let r = rig("");
    game_10_running(&r.proc);
    let (st, v) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced","appid":10}"#)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        eventually(|| r
            .sway_cmds()
            .iter()
            .any(|c| c.ends_with("[con_id=16] focus, fullscreen enable")))
        .await,
        "{:?}",
        r.sway_cmds()
    );
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        r.steam_calls().is_empty(),
        "relaunched: {:?}",
        r.steam_calls()
    );
    // Only the game's window: never Big Picture (steam, the reaper's parent).
    assert_eq!(r.sway_cmds().len(), 1, "{:?}", r.sway_cmds());
}

#[tokio::test]
async fn fresh_launch_fullscreens_the_window_once_it_appears() {
    let r = rig("");
    let (st, v) = r
        .call("POST", "/api/stream", r#"{"preset":"balanced","appid":10}"#)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(eventually(|| r.steam_calls().len() == 1).await);
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(
        r.sway_cmds().is_empty(),
        "no game window yet: {:?}",
        r.sway_cmds()
    );
    game_10_running(&r.proc); // steam started it a little later
    assert!(
        eventually(|| r
            .sway_cmds()
            .iter()
            .any(|c| c.ends_with("[con_id=16] focus, fullscreen enable")))
        .await,
        "{:?}",
        r.sway_cmds()
    );
}

#[tokio::test]
async fn games_marks_running_game() {
    let mut s = steam("http://127.0.0.1:1", "http://127.0.0.1:1", None);
    let p = s.cfg.data_dir.join("proc");
    fake_procs(
        &p,
        &[
            (
                500,
                1,
                &["/steam/reaper", "SteamLaunch", "AppId=620", "--", "x"],
            ),
            (7, 1, &["AppId=1091500"]),
        ],
    );
    s.cfg.proc_dir = p;
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(find(&v, 620)["running"], true);
    assert_eq!(find(&v, 1091500)["running"], false);
}
