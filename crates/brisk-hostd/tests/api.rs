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

use std::path::Path;
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
