//! Stage 3 routes and pipeline against a loopback SteamGridDB mock. Nothing here touches the net.
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::art::Art;
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use brisk_hostd::steamgriddb::SgdbConfig;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const SECRET: &str = "s3cret";
const KEY: &str = "SENTINELKEY-0123abcd";
const ID: &str = "emu:dolphin:RMCE01";

type Routes = Vec<(&'static str, u16, Vec<u8>)>;
struct Mock {
    base: String,
    log: Arc<Mutex<Vec<String>>>,
    hits: Arc<AtomicUsize>,
}
impl Mock {
    fn count(&self, s: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains(s))
            .count()
    }
}

/// `routes` gets the mock's own base URL (art URLs in replies point back at it).
async fn mock(routes: impl FnOnce(&str) -> Routes) -> Mock {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let routes = routes(&base);
    let log = Arc::new(Mutex::new(Vec::new()));
    let hits = Arc::new(AtomicUsize::new(0));
    let (lg, h) = (log.clone(), hits.clone());
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let (mut c, _) = l.accept().await.unwrap();
            h.fetch_add(1, Ordering::SeqCst);
            let mut buf = vec![0u8; 8192];
            let n = c.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = head.lines().next().unwrap_or("").to_string();
            lg.lock().unwrap().push(head);
            let (code, body) = routes
                .iter()
                .find(|(p, ..)| line.contains(p))
                .map_or((404, vec![]), |(_, c, b)| (*c, b.clone()));
            let r = format!(
                "HTTP/1.1 {code} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = c.write_all(r.as_bytes()).await;
            let _ = c.write_all(&body).await;
        }
    });
    Mock { base, log, hits }
}

fn png() -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    v.extend_from_slice(&[7u8; 64]);
    v
}
fn ok(v: Value) -> (u16, Vec<u8>) {
    (200, v.to_string().into_bytes())
}
fn empty() -> (u16, Vec<u8>) {
    ok(json!({"success": true, "data": []}))
}

/// SteamGridDB for "Mario Kart Wii": `search_body` for the search, one grid image, no other art.
fn sgdb(search_body: Value) -> impl FnOnce(&str) -> Routes {
    move |base| {
        let (c, b) = ok(search_body);
        let (gc, gb) = ok(
            json!({"success": true, "data": [{"url": format!("{base}/img/cover"), "width": 600, "height": 900, "style": "alternate", "mime": "image/png", "score": 3}]}),
        );
        let (ec, eb) = empty();
        vec![
            ("search/autocomplete/", c, b),
            ("grids/game/", gc, gb),
            ("heroes/game/", ec, eb.clone()),
            ("logos/game/", ec, eb.clone()),
            ("icons/game/", ec, eb),
            ("/img/cover", 200, png()),
        ]
    }
}

fn hit(id: u64, name: &str) -> Value {
    json!({"id": id, "name": name, "verified": false, "release_date": 1209600000})
}

struct Rig {
    _t: tempfile::TempDir,
    app: Router,
    state: Arc<AppState>,
    data: std::path::PathBuf,
}

fn rig(base: Option<&str>) -> Rig {
    rig_gap(base, Duration::ZERO)
}

fn rig_gap(base: Option<&str>, gap: Duration) -> Rig {
    let t = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env(t.path().join("dist"));
    cfg.secret = SECRET.into();
    cfg.games_dir = t.path().join("Games");
    cfg.data_dir = t.path().join("data");
    cfg.proc_dir = t.path().join("proc");
    cfg.steam_dir = t.path().join("nosteam");
    let data = cfg.data_dir.clone();
    std::fs::create_dir_all(&data).unwrap();
    let kf = t.path().join("key");
    std::fs::write(&kf, format!("{KEY}\n")).unwrap();
    let base = base.map(String::from);
    let mut st = AppState::new(cfg);
    st.scan_gap = Duration::ZERO;
    let mut art = Art::new(
        Box::new(move |d| {
            let mut c = SgdbConfig::with_key_file(&kf, base.clone()?, d)?;
            c.min_interval = Duration::ZERO;
            Some(c)
        }),
        |_, _| None,
    );
    art.refresh_gap = gap;
    st.art = Arc::new(art);
    let state = Arc::new(st);
    Rig {
        _t: t,
        app: router(state.clone()),
        state,
        data,
    }
}

fn disc(id: &[u8; 6], title: &str) -> Vec<u8> {
    let mut h = vec![0u8; 300];
    h[..6].copy_from_slice(id);
    h[0x18..0x1c].copy_from_slice(&[0x5D, 0x1C, 0x9E, 0xA3]);
    h[0x20..0x20 + title.len()].copy_from_slice(title.as_bytes());
    h
}

impl Rig {
    async fn raw(
        &self,
        method: &str,
        uri: &str,
        hdr: &[(&str, &str)],
        body: Vec<u8>,
        auth: bool,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut r = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "localhost:8443");
        if auth {
            r = r.header("x-brisk-secret", SECRET);
        }
        for (k, v) in hdr {
            r = r.header(*k, *v);
        }
        let r = self
            .app
            .clone()
            .oneshot(r.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let (st, h) = (r.status(), r.headers().clone());
        (
            st,
            h,
            axum::body::to_bytes(r.into_body(), 1 << 20)
                .await
                .unwrap()
                .to_vec(),
        )
    }
    async fn call(&self, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let b = if body.is_null() {
            vec![]
        } else {
            body.to_string().into_bytes()
        };
        let (s, _, b) = self.raw(method, uri, &[], b, true).await;
        (s, serde_json::from_slice(&b).unwrap_or_default())
    }
    /// Uploads a disc image and finishes it: the new game triggers the lookup.
    async fn add(&self, id: &[u8; 6], title: &str) {
        let bytes = disc(id, title);
        let (_, v) = self
            .call(
                "POST",
                "/api/uploads",
                json!({"name": "x.iso", "size": bytes.len()}),
            )
            .await;
        let up = v["id"].as_str().unwrap().to_string();
        let (s, _, _) = self
            .raw(
                "PUT",
                &format!("/api/uploads/{up}"),
                &[("x-offset", "0")],
                bytes,
                true,
            )
            .await;
        assert_eq!(s, StatusCode::OK);
        let (s, v) = self
            .call("POST", &format!("/api/uploads/{up}/finish"), Value::Null)
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    async fn game(&self) -> Value {
        let (_, g) = self.call("GET", "/api/games", Value::Null).await;
        g["games"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == ID)
            .cloned()
            .unwrap_or_default()
    }
    /// Waits for the background task to leave.
    async fn idle(&self) {
        let id = brisk_hostd::game::GameId::parse(ID).unwrap();
        for _ in 0..200 {
            tokio::time::sleep(Duration::from_millis(25)).await;
            if !self.state.art.busy(&id) {
                return;
            }
        }
        panic!("art task never finished");
    }
}

#[tokio::test]
async fn confident_match_fetches_once_after_an_upload() {
    let m = mock(sgdb(
        json!({"success": true, "data": [hit(5, "Mario Kart Wii"), hit(6, "Other Thing")]}),
    ))
    .await;
    let r = rig(Some(&m.base));
    r.add(b"RMCE01", "Mario Kart Wii").await;
    r.idle().await;
    let g = r.game().await;
    assert!(
        g["cover"]
            .as_str()
            .unwrap()
            .starts_with("/api/games/emu%3Adolphin%3ARMCE01/art/cover?v="),
        "{g}"
    );
    assert_eq!(g["hero"], "");
    assert_eq!(g["credits"], json!(["SteamGridDB"]));
    assert_eq!(g["details"]["year"], 2008);
    assert_eq!(g["caps"]["art"], true);
    assert!(g.get("needsMatch").is_none());
    // the search and the image requests never carry the key in the URL
    assert!(
        m.log
            .lock()
            .unwrap()
            .iter()
            .all(|h| !h.lines().next().unwrap().contains(KEY))
    );
    assert!(
        m.log
            .lock()
            .unwrap()
            .iter()
            .filter(|h| h.contains("/img/"))
            .all(|h| !h.contains(KEY))
    );
    // an image, with type, etag and a long cache
    let uri = g["cover"].as_str().unwrap();
    let (s, h, b) = r.raw("GET", uri, &[], vec![], true).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h["content-type"], "image/png");
    assert!(
        h["cache-control"]
            .to_str()
            .unwrap()
            .contains("max-age=31536000")
    );
    assert_eq!(b, png());
    let etag = h["etag"].to_str().unwrap().to_string();
    let (s, _, b) = r
        .raw("GET", uri, &[("if-none-match", &etag)], vec![], true)
        .await;
    assert_eq!((s, b.len()), (StatusCode::NOT_MODIFIED, 0));
    // a second run asks nobody
    let before = m.hits.load(Ordering::SeqCst);
    let id = brisk_hostd::game::GameId::parse(ID).unwrap();
    let out = r
        .state
        .art
        .run(&r.state.cfg, &id, "Mario Kart Wii", Some("wii"), None);
    assert_eq!(out, brisk_hostd::art::Outcome::Cached);
    assert_eq!(m.hits.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn weak_match_waits_for_a_pick_and_fetches_nothing() {
    let m = mock(sgdb(json!({"success": true, "data": [hit(1, "Mario Kart 8"), hit(2, "Mario Party"), hit(3, "Mario Tennis"), hit(4, "Mario Golf Mario")]}))).await;
    let r = rig(Some(&m.base));
    r.add(b"RMCE01", "Mario Kart Wii").await;
    r.idle().await;
    assert_eq!(m.count("/img/"), 0);
    assert_eq!(m.count("grids/game"), 0);
    let g = r.game().await;
    assert_eq!(
        (g["needsMatch"].as_bool(), g["cover"].as_str()),
        (Some(true), Some(""))
    );
    let (s, p) = r.call("GET", "/api/art/pending", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["hasKey"], true);
    assert_eq!(p["credits"], json!(["SteamGridDB"]));
    let it = &p["items"][0];
    assert_eq!(
        (it["id"].as_str(), it["title"].as_str()),
        (Some(ID), Some("Mario Kart Wii"))
    );
    assert_eq!(it["candidates"].as_array().unwrap().len(), 3);
    assert!(!p.to_string().contains(KEY));
    let mode = std::fs::metadata(r.data.join("art/pending.json")).unwrap();
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
        0o600
    );
    // validation
    let url = format!("/api/art/{ID}/match");
    for body in [
        json!({}),
        json!({"candidateId": 1, "none": true}),
        json!({"candidateId": "x"}),
        json!({"candidateId": 1, "x": 1}),
    ] {
        assert_eq!(
            r.call("POST", &url, body.clone()).await.0,
            StatusCode::BAD_REQUEST,
            "{body}"
        );
    }
    assert_eq!(
        r.call("POST", &url, json!({"candidateId": 99})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        r.call(
            "POST",
            "/api/art/emu:dolphin:NOPE/match",
            json!({"none": true})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        r.call("POST", "/api/art/steam:620/match", json!({"none": true}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // confirm: saves the match, fetches
    let (s, _) = r.call("POST", &url, json!({"candidateId": 2})).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    r.idle().await;
    assert_eq!(m.count("grids/game/2"), 1);
    let g = r.game().await;
    assert!(g["cover"].as_str().unwrap().contains("/art/cover"), "{g}");
    assert!(g.get("needsMatch").is_none());
    assert_eq!(
        r.call("GET", "/api/art/pending", Value::Null).await.1["items"],
        json!([])
    );
    // answered once: a second answer has nothing to answer
    assert_eq!(
        r.call("POST", &url, json!({"candidateId": 2})).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn none_of_these_is_cached_until_refresh() {
    let m = mock(sgdb(
        json!({"success": true, "data": [hit(1, "Mario Kart 8"), hit(2, "Mario Party")]}),
    ))
    .await;
    let r = rig(Some(&m.base));
    r.add(b"RMCE01", "Mario Kart Wii").await;
    r.idle().await;
    let url = format!("/api/art/{ID}/match");
    assert_eq!(
        r.call("POST", &url, json!({"none": true})).await.1["state"],
        "none"
    );
    assert_eq!(
        r.call("GET", "/api/art/pending", Value::Null).await.1["items"],
        json!([])
    );
    // not asked again: a run makes no request
    let (id, before) = (
        brisk_hostd::game::GameId::parse(ID).unwrap(),
        m.hits.load(Ordering::SeqCst),
    );
    assert_eq!(
        r.state
            .art
            .run(&r.state.cfg, &id, "Mario Kart Wii", Some("wii"), None),
        brisk_hostd::art::Outcome::NoMatch
    );
    assert_eq!(m.hits.load(Ordering::SeqCst), before);
    // refresh clears it and searches again: back in the queue
    let (s, _) = r
        .call("POST", &format!("/api/art/{ID}/refresh"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    r.idle().await;
    assert_eq!(m.count("search/autocomplete/"), 2);
    assert_eq!(
        r.call("GET", "/api/art/pending", Value::Null).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn refresh_needs_a_key() {
    let k = rig(None);
    k.add(b"RMCE01", "Mario Kart Wii").await;
    let (s, v) = k
        .call("POST", &format!("/api/art/{ID}/refresh"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(v["error"].as_str().unwrap().contains("SteamGridDB key"));
    assert_eq!(
        k.call("GET", "/api/art/pending", Value::Null).await.1["hasKey"],
        false
    );
    assert_eq!(k.game().await["cover"], "");
}

#[tokio::test]
async fn refresh_twice_quickly_is_limited() {
    let m = mock(sgdb(
        json!({"success": true, "data": [hit(5, "Mario Kart Wii")]}),
    ))
    .await;
    let r = rig_gap(Some(&m.base), Duration::from_secs(60));
    r.add(b"RMCE01", "Mario Kart Wii").await;
    r.idle().await;
    let url = format!("/api/art/{ID}/refresh");
    assert_eq!(
        r.call("POST", &url, Value::Null).await.0,
        StatusCode::ACCEPTED
    );
    r.idle().await;
    let (s, h, _) = r.raw("POST", &url, &[], vec![], true).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert!(h.contains_key("retry-after"));
}

#[tokio::test]
async fn errors_store_nothing_and_never_leak_the_key() {
    for code in [401u16, 429, 500] {
        let m = mock(move |_| vec![("search/autocomplete/", code, b"{}".to_vec())]).await;
        let r = rig(Some(&m.base));
        r.add(b"RMCE01", "Mario Kart Wii").await;
        r.idle().await;
        let g = r.game().await;
        assert_eq!(
            (g["cover"].as_str(), g.get("needsMatch")),
            (Some(""), None),
            "{code}"
        );
        assert!(!r.data.join("art/pending.json").exists());
        assert!(!r.data.join("art/emu_dolphin_RMCE01/nomatch").exists());
        assert!(!g.to_string().contains(KEY));
        assert_eq!(m.count("search/autocomplete/"), 1, "no retry loop");
    }
}

#[tokio::test]
async fn an_image_host_off_the_allowlist_is_not_fetched() {
    let m = mock(|base| {
        let (c, b) = ok(json!({"success": true, "data": [hit(5, "Mario Kart Wii")]}));
        let (gc, gb) = ok(json!({"success": true, "data": [{"url": "https://evil.example/x.png", "width": 600, "height": 900, "style": "a", "mime": "image/png", "score": 1}]}));
        let _ = base;
        vec![("search/autocomplete/", c, b), ("grids/game/", gc, gb), ("game/", 200, empty().1)]
    })
    .await;
    let r = rig(Some(&m.base));
    r.add(b"RMCE01", "Mario Kart Wii").await;
    r.idle().await;
    assert_eq!(r.game().await["cover"], "");
}

#[tokio::test]
async fn image_route_checks_auth_ids_and_kinds() {
    let m = mock(sgdb(
        json!({"success": true, "data": [hit(5, "Mario Kart Wii")]}),
    ))
    .await;
    let r = rig(Some(&m.base));
    r.add(b"RMCE01", "Mario Kart Wii").await;
    r.idle().await;
    let ok_uri = "/api/games/emu%3Adolphin%3ARMCE01/art/cover";
    assert_eq!(
        r.raw("GET", ok_uri, &[], vec![], false).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        r.raw("GET", ok_uri, &[], vec![], true).await.0,
        StatusCode::OK
    );
    for (uri, want) in [
        (
            "/api/games/emu%3Adolphin%3ARMCE01/art/hero",
            StatusCode::NOT_FOUND,
        ),
        (
            "/api/games/emu%3Adolphin%3ARMCE01/art/..%2Fmatch.json",
            StatusCode::NOT_FOUND,
        ),
        (
            "/api/games/emu%3Adolphin%3ARMCE01/art/cover.png",
            StatusCode::NOT_FOUND,
        ),
        ("/api/games/..%2F..%2Fx/art/cover", StatusCode::BAD_REQUEST),
        (
            "/api/games/emu%3Adolphin%3A..%2F..%2Fx/art/cover",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/games/emu%3Adolphin%3A%2E%2E/art/cover",
            StatusCode::NOT_FOUND,
        ),
        ("/api/games/620/art/cover", StatusCode::BAD_REQUEST),
        ("/api/games/steam%3A620/art/cover", StatusCode::BAD_REQUEST),
        (
            "/api/games/emu%3Adolphin%3AOTHER1/art/cover",
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(r.raw("GET", uri, &[], vec![], true).await.0, want, "{uri}");
    }
    // a non-image file in the place of an image is not served
    std::fs::write(r.data.join("art/emu_dolphin_RMCE01/cover.png"), b"<html>").unwrap();
    assert_eq!(
        r.raw("GET", ok_uri, &[], vec![], true).await.0,
        StatusCode::NOT_FOUND
    );
    // routes need the secret
    for (m, u) in [
        ("GET", "/api/art/pending"),
        ("POST", "/api/art/emu:dolphin:RMCE01/match"),
        ("POST", "/api/art/emu:dolphin:RMCE01/refresh"),
    ] {
        assert_eq!(
            r.raw(m, u, &[], vec![], false).await.0,
            StatusCode::FORBIDDEN,
            "{u}"
        );
    }
}

#[tokio::test]
async fn one_task_per_game_at_a_time() {
    let m = mock(sgdb(
        json!({"success": true, "data": [hit(5, "Mario Kart Wii")]}),
    ))
    .await;
    let r = rig(Some(&m.base));
    r.add(b"RMCE01", "Mario Kart Wii").await;
    let id = brisk_hostd::game::GameId::parse(ID).unwrap();
    r.idle().await;
    let a = r.state.art.spawn(
        r.state.cfg.clone(),
        id.clone(),
        "Mario Kart Wii".into(),
        None,
        None,
    );
    let b = r
        .state
        .art
        .spawn(r.state.cfg.clone(), id, "Mario Kart Wii".into(), None, None);
    assert!(a && !b);
    r.idle().await;
}
