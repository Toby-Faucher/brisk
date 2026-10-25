use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use brisk_hostd::config::Config;
use brisk_hostd::http::{router, AppState};
use std::sync::Arc;
use tower::ServiceExt;

const SECRET: &str = "s3cret";

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

const H: &str = "localhost:8443";

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
        send(d.path(), "GET", "/api/nope", H, None).await.status(),
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
async fn get_needs_no_secret() {
    let d = dist();
    assert_eq!(
        send(d.path(), "GET", "/api/unknown", H, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

// ---- Steam library ----

use std::path::{Path, PathBuf};
use std::sync::Mutex;

const KEY: &str = "TESTKEY-0123456789abcdef";
const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/steam");

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

/// Mock Steam API: 200 with the fixture only when the sentinel key is in the query, else 403.
async fn mock_steam(status: u16) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let (mut c, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = c.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let ok = head.contains("/IPlayerService/GetOwnedGames/v1/")
                && head.contains(&format!("key={KEY}"))
                && head.contains("include_appinfo=1")
                && head.contains("include_played_free_games=1");
            let (code, body) = if status != 200 {
                (status, String::new())
            } else if ok {
                (
                    200,
                    std::fs::read_to_string(format!("{FIX}/owned.json")).unwrap(),
                )
            } else {
                (403, String::new())
            };
            let r = format!(
                "HTTP/1.1 {code} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = c.write_all(r.as_bytes()).await;
        }
    });
    base
}

struct Steam {
    _t: tempfile::TempDir,
    cfg: Config,
}

fn steam(base: &str, key: Option<&str>) -> Steam {
    let t = tempfile::tempdir().unwrap();
    let (s, l2) = (t.path().join("steam"), t.path().join("lib2"));
    copy_dir(
        Path::new(FIX).join("lib1/steamapps").as_path(),
        &s.join("steamapps"),
    );
    copy_dir(
        Path::new(FIX).join("lib2/steamapps").as_path(),
        &l2.join("steamapps"),
    );
    let vdf = std::fs::read_to_string(format!("{FIX}/libraryfolders.vdf.tmpl"))
        .unwrap()
        .replace("@LIB1@", s.to_str().unwrap())
        .replace("@LIB2@", l2.to_str().unwrap());
    std::fs::write(s.join("steamapps/libraryfolders.vdf"), vdf).unwrap();
    std::fs::create_dir_all(s.join("config")).unwrap();
    std::fs::write(
        s.join("config/loginusers.vdf"),
        "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"MostRecent\"\t\"1\"\n\t}\n}\n",
    )
    .unwrap();
    let mut cfg = Config::from_env(t.path().join("dist"));
    cfg.secret = SECRET.into();
    cfg.steam_dir = s;
    cfg.data_dir = t.path().join("data");
    cfg.steam_api_base = base.into();
    cfg.steam_key_file = t.path().join("key");
    if let Some(k) = key {
        std::fs::write(&cfg.steam_key_file, format!("{k}\n")).unwrap();
    }
    Steam { _t: t, cfg }
}

async fn games_json(cfg: &Config, uri: &str) -> (serde_json::Value, String) {
    let app = router(Arc::new(AppState::new(cfg.clone())));
    let r = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("host", H)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let b = body(r).await;
    (serde_json::from_str(&b).unwrap(), b)
}

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
    let s = steam("http://127.0.0.1:1", None);
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
    let s = steam("http://127.0.0.1:1", None);
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    let g = find(&v, 730);
    assert_eq!(g["installed"], false);
    assert!(g["sizeBytes"].is_null());
}

#[tokio::test]
async fn owned_games_merged_with_installed() {
    let s = steam(&mock_steam(200).await, Some(KEY));
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
    let s = steam("http://127.0.0.1:1", None);
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
    let s = steam(&mock_steam(500).await, Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(v["ownedAvailable"], false);
    assert_eq!(v["games"].as_array().unwrap().len(), 3);
    assert_eq!(find(&v, 620)["installed"], true);
}

#[tokio::test]
async fn missing_key_file_is_not_an_error() {
    let s = steam(&mock_steam(200).await, None);
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(v["ownedAvailable"], false);
    assert_eq!(v["games"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn owned_cache_is_used_until_refresh() {
    let s = steam(&mock_steam(200).await, Some(KEY));
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
    let s = steam(&mock_steam(200).await, Some(KEY));
    let st = AppState::new(s.cfg.clone());
    assert!(st.is_known_appid(570).await);
    assert!(!st.is_known_appid(999).await);
}

struct Capture(Arc<Mutex<String>>);
impl tracing::field::Visit for Capture {
    fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
        self.0
            .lock()
            .unwrap()
            .push_str(&format!("{}={v:?} ", f.name()));
    }
}
struct Sub(Arc<Mutex<String>>);
impl tracing::Subscriber for Sub {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        self.0.lock().unwrap().push_str(e.metadata().target());
        e.record(&mut Capture(self.0.clone()));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn key_never_in_response_or_logs() {
    // Global (not thread-local) so other tests' callsite caches cannot hide our events.
    let logs = Arc::new(Mutex::new(String::new()));
    tracing::subscriber::set_global_default(Sub(logs.clone())).unwrap();
    let mut out = String::new();
    for status in [200, 500] {
        let s = steam(&mock_steam(status).await, Some(KEY));
        out += &games_json(&s.cfg, "/api/games").await.1;
        out += &format!("{:?}", s.cfg);
    }
    assert!(!out.is_empty() && !out.contains(KEY) && !out.contains("0123456789abcdef"));
    let logs = logs.lock().unwrap();
    assert!(
        logs.contains("GetOwnedGames failed"),
        "expected a log line: {logs}"
    );
    assert!(!logs.contains("0123456789abcdef"));
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
    assert!(body(r).await.contains("error"));
    assert_eq!(*runner.0.lock().unwrap(), 1);
    let r = app
        .oneshot(
            Request::builder()
                .uri("/api/host")
                .header("host", H)
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

const FAKE_BRIDGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake-bridge.sh");

/// `run` is slow on purpose: steam commands must use the detached `spawn`, never wait.
struct Calls(std::sync::Mutex<Vec<String>>);
impl brisk_hostd::host::Runner for Calls {
    fn run(&self, p: &str, a: &[&str]) -> Option<String> {
        std::thread::sleep(std::time::Duration::from_secs(2));
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
    app: axum::Router,
    runner: Arc<Calls>,
    log: PathBuf,
    data: PathBuf,
    _d: tempfile::TempDir,
}

fn rig(mode: &str) -> Rig {
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
    cfg.bridge_listen = "100.64.0.1:4433".into();
    cfg.app_id = 42;
    cfg.data_dir = d.path().join("data");
    cfg.steam_dir = d.path().join("nosteam");
    cfg.steam_key_file = d.path().join("nokey");
    cfg.steam_api_base = "http://127.0.0.1:1".into();
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
            cover: String::new(),
        }],
        owned_available: true,
    });
    Rig {
        app: router(Arc::new(st)),
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
    assert_eq!(r.call("GET", "/api/stream", "").await.1, v);
    let log = r.log();
    assert!(
        log[0].contains("--host 127.0.0.1 --app 42 --listen 100.64.0.1:4433 --cert ")
            && log[0].contains("/.config/brisk/tls/bridge.pem --key ")
            && log[0].contains("/.config/brisk/tls/bridge.key.pem --page http://localhost:8443/"),
        "{log:?}"
    );
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
    assert!(r.call("GET", "/api/sessions", "").await.1["sessions"]
        .as_array()
        .unwrap()
        .is_empty());
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
}

#[tokio::test]
async fn sessions_returns_latest_first() {
    let r = rig("");
    std::fs::create_dir_all(&r.data).unwrap();
    let mut lines = String::new();
    for i in 1..=25u64 {
        let ended = if i == 25 {
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
    // the open record of 25 is closed by a later line with the same start
    lines +=
        r#"{"appid":25,"name":"g25","started":25000,"ended":25009,"host":"bawn","preset":"fast"}"#;
    lines += "\n";
    std::fs::write(r.data.join("sessions.jsonl"), lines).unwrap();
    let s = r.call("GET", "/api/sessions", "").await.1;
    let s = s["sessions"].as_array().unwrap();
    assert_eq!(s.len(), 20);
    assert_eq!(s[0]["started"], 25000);
    assert_eq!(s[0]["ended"], 25009);
    assert_eq!(s[1]["started"], 24000);
    assert_eq!(s[19]["started"], 6000);
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
    let s = steam(&mock_steam(200).await, Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert!(v["games"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g["appid"] != 1628350 && g["appid"] != 1493710));
    // Without it: dropped by name prefix, real games stay.
    let s = steam(&mock_steam(500).await, Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    assert_eq!(v["ownedAvailable"], false);
    assert_eq!(v["games"].as_array().unwrap().len(), 3);
    assert!(v["games"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g["appid"] != 1628350 && g["appid"] != 1493710));
}
