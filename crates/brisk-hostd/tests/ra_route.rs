//! Emulator-game achievements through the router, against a loopback RetroAchievements mock.
//! The disc is a synthetic GameCube ISO (the Wii hash is only checked against a synthetic image too).
mod common;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use common::{H, SECRET};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const KEY: &str = "SECRETKEY123";
const ID: &str = "emu:dolphin:gcgame";

type Log = Arc<Mutex<Vec<String>>>;

async fn mock(routes: Vec<(&'static str, u16, String)>) -> (String, Log) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let lg = log.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let (mut c, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = c.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = head.lines().next().unwrap_or("").to_string();
            lg.lock().unwrap().push(line.clone());
            let (code, body) = match routes.iter().find(|(p, _, _)| line.contains(p)) {
                Some((_, c, b)) => (*c, b.clone()),
                None => (404, String::new()),
            };
            let h = format!(
                "HTTP/1.1 {code} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = c.write_all(h.as_bytes()).await;
        }
    });
    (base, log)
}

fn put(img: &mut [u8], at: usize, v: &[u8]) {
    img[at..at + v.len()].copy_from_slice(v);
}

/// Same layout as the rahash unit test image.
fn gc_image() -> Vec<u8> {
    let mut img = vec![0u8; 0x3300];
    put(&mut img, 0, b"GTESTA");
    put(&mut img, 0x1C, &[0xC2, 0x33, 0x9F, 0x3D]);
    put(&mut img, 0x20, b"Synthetic");
    put(&mut img, 0x420, &0x3000u32.to_be_bytes());
    put(&mut img, 0x2454, &0x30u32.to_be_bytes());
    put(&mut img, 0x2458, &0x10u32.to_be_bytes());
    put(&mut img, 0x3000, &0x3100u32.to_be_bytes());
    put(&mut img, 0x3000 + 0x90, &5u32.to_be_bytes());
    put(&mut img, 0x3100, b"CODE!");
    img
}

struct Rig {
    _t: tempfile::TempDir,
    cfg: Config,
    iso: std::path::PathBuf,
}

/// A library of one GameCube game (`system` "gamecube"), RA key file present unless `creds` is None.
fn rig(base: &str, creds: Option<&str>, system: &str) -> Rig {
    let t = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env(t.path().join("dist"));
    cfg.secret = SECRET.into();
    cfg.data_dir = t.path().join("data");
    cfg.steam_dir = t.path().join("steam");
    cfg.proc_dir = t.path().join("proc");
    std::fs::create_dir_all(&cfg.proc_dir).unwrap();
    cfg.steam_key_file = t.path().join("nokey");
    cfg.steam_api_base = "http://127.0.0.1:1".into();
    cfg.ra_key_file = t.path().join("ra-key");
    cfg.ra_base = base.into();
    cfg.ra_min_interval = std::time::Duration::ZERO;
    if let Some(c) = creds {
        std::fs::write(&cfg.ra_key_file, c).unwrap();
    }
    let iso = t.path().join("game.iso");
    std::fs::write(&iso, gc_image()).unwrap();
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    std::fs::write(
        cfg.data_dir.join("library.json"),
        serde_json::json!({"v":1,"games":[{"id":ID,"emulator":"dolphin","system":system,"file":iso,"name":"GC Game"}]})
            .to_string(),
    )
    .unwrap();
    Rig { _t: t, cfg, iso }
}

fn app(cfg: &Config) -> Router {
    router(Arc::new(AppState::new(cfg.clone())))
}

async fn get(app: &Router, uri: &str, secret: bool) -> (StatusCode, Option<String>, String) {
    let mut r = Request::builder().uri(uri).header("host", H);
    if secret {
        r = r.header("x-brisk-secret", SECRET);
    }
    let r = app
        .clone()
        .oneshot(r.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cc = r
        .headers()
        .get("cache-control")
        .map(|v| v.to_str().unwrap().to_string());
    let st = r.status();
    let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    (st, cc, String::from_utf8(b.to_vec()).unwrap())
}

async fn json(app: &Router, uri: &str) -> serde_json::Value {
    serde_json::from_str(&get(app, uri, true).await.2).unwrap()
}

fn hash_of(r: &Rig) -> String {
    brisk_hostd::rahash::ra_hash_gamecube_iso(&r.iso).unwrap()
}

fn list(hash: &str) -> String {
    format!(
        r#"[{{"Title":"GC Game","ID":777,"ConsoleID":16,"ConsoleName":"GameCube","Hashes":["{hash}"]}}]"#
    )
}

fn progress() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ra/progress-38107.json"
    ))
    .unwrap()
}

const GOOD: &str = "toby_ra\nSECRETKEY123\n";
const URI: &str = "/api/games/emu:dolphin:gcgame/achievements";

async fn ok_rig() -> (Rig, Router, Log) {
    let r0 = rig("http://127.0.0.1:1", Some(GOOD), "gamecube");
    let routes = vec![
        ("API_GetGameList.php", 200, list(&hash_of(&r0))),
        ("API_GetGameInfoAndUserProgress.php", 200, progress()),
    ];
    let (base, log) = mock(routes).await;
    let mut r = r0;
    r.cfg.ra_base = base;
    let a = app(&r.cfg);
    (r, a, log)
}

#[tokio::test]
async fn ok_has_the_steam_shape_plus_ra_fields() {
    let (_r, a, _) = ok_rig().await;
    let (st, cc, body) = get(&a, URI, true).await;
    assert_eq!((st, cc.as_deref()), (StatusCode::OK, Some("no-store")));
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["state"], "ok");
    assert_eq!(v["source"], "retroachievements");
    assert_eq!(v["matched"], "hash");
    assert_eq!(
        (
            v["available"].clone(),
            v["unlocked"].clone(),
            v["total"].clone()
        ),
        (true.into(), 1.into(), 2.into())
    );
    assert_eq!(
        (v["pointsUnlocked"].clone(), v["pointsTotal"].clone()),
        (3.into(), 8.into())
    );
    let first = &v["list"][0];
    assert_eq!(first["id"], "21");
    assert_eq!(first["name"], "First");
    assert_eq!(first["description"], "Do the first thing");
    assert_eq!(
        first["icon"],
        "https://media.retroachievements.org/Badge/250336.png"
    );
    assert_eq!(first["unlockedAt"], 1773337649u64);
    assert_eq!(
        (first["points"].clone(), first["hardcore"].clone()),
        (3.into(), true.into())
    );
    let second = &v["list"][1];
    assert!(second["unlockedAt"].is_null());
    assert_eq!(
        second["icon"],
        "https://media.retroachievements.org/Badge/250337_lock.png"
    );
    // Same keys per entry as Steam's Ach.
    for k in ["id", "name", "description", "icon", "unlockedAt"] {
        assert!(first.get(k).is_some());
    }
    assert!(!body.contains(KEY) && !body.contains("toby_ra"));
}

#[tokio::test]
async fn states_off_unsupported_not_found_and_upstream_errors() {
    // off: no key file, no outbound call
    let (base, log) = mock(vec![]).await;
    let r = rig(&base, None, "gamecube");
    assert_eq!(json(&app(&r.cfg), URI).await["state"], "off");
    assert!(log.lock().unwrap().is_empty());

    // unsupported: an RVZ header
    let (base, _) = mock(vec![]).await;
    let r = rig(&base, Some(GOOD), "gamecube");
    let mut rvz = vec![0u8; 0x200];
    put(&mut rvz, 0, b"RVZ\x01");
    put(&mut rvz, 0x58, b"GTESTA");
    put(&mut rvz, 0x58 + 0x1C, &[0xC2, 0x33, 0x9F, 0x3D]);
    std::fs::write(&r.iso, rvz).unwrap();
    let v = json(&app(&r.cfg), URI).await;
    assert_eq!(v["state"], "unsupported");
    assert!(v["reason"].as_str().unwrap().contains("Rvz"));

    // not-found: hash not in the console's list
    let r0 = rig("http://127.0.0.1:1", Some(GOOD), "gamecube");
    let (base, _) = mock(vec![(
        "API_GetGameList.php",
        200,
        list("ffffffffffffffffffffffffffffffff").replace("GC Game", "Other Game"),
    )])
    .await;
    let mut r = r0;
    r.cfg.ra_base = base;
    let v = json(&app(&r.cfg), URI).await;
    assert_eq!(v["state"], "not-found");
    assert!(v["reason"].as_str().unwrap().contains("hash"));

    // upstream failures, malformed credentials and body
    let cases: [(u16, &str, &str); 4] = [
        (401, "{}", "bad-credentials"),
        (429, "{}", "rate-limited"),
        (500, "{}", "error"),
        (200, "not json", "error"),
    ];
    for (code, body, want) in cases {
        let (base, _) = mock(vec![("API_GetGameList.php", code, body.into())]).await;
        let mut r = rig(&base, Some(GOOD), "gamecube");
        r.cfg.ra_base = base;
        let (_, _, text) = get(&app(&r.cfg), URI, true).await;
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["state"], want, "{code}");
        assert!(v.as_object().unwrap().len() == 1, "no detail leaks: {text}");
        assert!(!text.contains(KEY) && !text.contains("127.0.0.1"));
    }
    let r = rig("http://127.0.0.1:1", Some("onlyuser\n"), "gamecube");
    assert_eq!(json(&app(&r.cfg), URI).await["state"], "bad-credentials");
}

#[tokio::test]
async fn refresh_is_honoured_once_per_30_seconds() {
    let (_r, a, log) = ok_rig().await;
    let count = || {
        log.lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains("API_GetGameInfoAndUserProgress"))
            .count()
    };
    json(&a, URI).await;
    json(&a, URI).await;
    assert_eq!(count(), 1, "cached for the TTL");
    json(&a, &format!("{URI}?refresh=1")).await;
    assert_eq!(count(), 2, "first refresh goes through");
    json(&a, &format!("{URI}?refresh=1")).await;
    assert_eq!(count(), 2, "second within 30 s is served from cache");
}

#[tokio::test]
async fn ra_hash_route_and_cache() {
    let (r, a, _) = ok_rig().await;
    let v = json(&a, "/api/games/emu:dolphin:gcgame/ra-hash").await;
    assert_eq!(v["hash"], hash_of(&r));
    assert_eq!(
        (v["console"].clone(), v["container"].clone()),
        (16.into(), "iso".into())
    );
    assert!(v["computedAt"].as_u64().unwrap() > 0);
    // unsupported container: 422, no hash
    std::fs::write(&r.iso, b"junk").unwrap();
    let (st, _, body) = get(&a, "/api/games/emu:dolphin:gcgame/ra-hash", true).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(!body.contains("hash"));
    // Steam games have none
    let (st, _, _) = get(&a, "/api/games/steam:620/ra-hash", true).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn auth_is_required() {
    let (_r, a, _) = ok_rig().await;
    for u in [URI, "/api/games/emu:dolphin:gcgame/ra-hash"] {
        assert_eq!(get(&a, u, false).await.0, StatusCode::FORBIDDEN, "{u}");
    }
}

#[tokio::test]
async fn caps_need_a_ra_system_and_the_key_file() {
    let caps = |r: &Rig| {
        let recs = brisk_hostd::emu::load(&r.cfg.data_dir);
        let mut g = brisk_hostd::emu::games(&recs, &r.cfg.proc_dir, &r.cfg.data_dir);
        brisk_hostd::ra_game::caps_on(&r.cfg, &mut g);
        g[0].caps.achievements
    };
    assert!(caps(&rig("http://x", Some(GOOD), "gamecube")));
    assert!(caps(&rig("http://x", Some(GOOD), "wii")));
    assert!(!caps(&rig("http://x", None, "wii")));
    assert!(!caps(&rig("http://x", Some(GOOD), "nes")));
}

/// Real-shaped list entries (ID, Title, ConsoleID, NumAchievements, Hashes).
fn entry(id: u64, title: &str, hash: &str) -> String {
    format!(
        r#"{{"Title":"{title}","ID":{id},"ConsoleID":16,"ConsoleName":"GameCube","NumAchievements":2,"Hashes":["{hash}"]}}"#
    )
}

const OTHER: &str = "ffffffffffffffffffffffffffffffff";

async fn matched_log(entries: Vec<String>) -> (serde_json::Value, Log) {
    let mut r = rig("http://127.0.0.1:1", Some(GOOD), "gamecube");
    let (base, log) = mock(vec![
        (
            "API_GetGameList.php",
            200,
            format!("[{}]", entries.join(",")),
        ),
        ("API_GetGameInfoAndUserProgress.php", 200, progress()),
    ])
    .await;
    r.cfg.ra_base = base;
    (json(&app(&r.cfg), URI).await, log)
}

async fn matched(entries: Vec<String>) -> serde_json::Value {
    matched_log(entries).await.0
}

#[tokio::test]
async fn title_fallback_when_the_hash_is_unlisted() {
    // exact match after normalising case, region tag and punctuation
    let v = matched(vec![
        entry(1, "gc game: (USA)", OTHER),
        entry(2, "Another", OTHER),
    ])
    .await;
    assert_eq!(
        (v["state"].as_str(), v["matched"].as_str()),
        (Some("ok"), Some("title"))
    );
    // no match
    let v = matched(vec![entry(1, "Different Game", OTHER)]).await;
    assert_eq!(v["state"], "not-found");
    assert!(v["reason"].as_str().unwrap().contains("hash"));
    // ambiguous: two games normalise to the name
    let v = matched(vec![
        entry(1, "GC Game", OTHER),
        entry(2, "GC Game (Europe)", OTHER),
    ])
    .await;
    assert_eq!(v["state"], "not-found");
    // a subset is a different game, and the library name may carry a file tag
    let v = matched(vec![
        entry(1, "GC Game", OTHER),
        entry(2, "GC Game [Subset - X]", OTHER),
    ])
    .await;
    assert_eq!(v["matched"], "title");
    let v = matched(vec![entry(2, "GC Game [Subset - X]", OTHER)]).await;
    assert_eq!(v["state"], "not-found");
}

#[tokio::test]
async fn hash_match_beats_a_title_match() {
    let r0 = rig("http://127.0.0.1:1", Some(GOOD), "gamecube");
    let h = hash_of(&r0);
    let (v, log) = matched_log(vec![entry(1, "GC Game", OTHER), entry(777, "Renamed", &h)]).await;
    assert_eq!(
        (v["state"].as_str(), v["matched"].as_str()),
        (Some("ok"), Some("hash"))
    );
    let log = log.lock().unwrap();
    assert!(log.iter().any(|l| l.contains("g=777")) && !log.iter().any(|l| l.contains("g=1 ")));
}
