//! Stage 2 routes: scan, resumable upload, remove. Design: docs/plans/2026-10-28-stage2-upload.md.
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use serde_json::{Value, json};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::ServiceExt;

const SECRET: &str = "s3cret";
const GB: u64 = 1 << 30;

thread_local! { static FREE: Cell<u64> = const { Cell::new(1 << 40) }; }
fn free(_: &Path) -> Option<u64> {
    Some(FREE.with(Cell::get))
}

struct Rig {
    _t: tempfile::TempDir,
    app: Router,
    games: PathBuf,
    data: PathBuf,
    state: Arc<AppState>,
}

fn rig() -> Rig {
    FREE.with(|f| f.set(1 << 40));
    let t = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env(t.path().join("dist"));
    cfg.secret = SECRET.into();
    cfg.games_dir = t.path().join("Games");
    cfg.data_dir = t.path().join("data");
    cfg.proc_dir = t.path().join("proc");
    let (games, data) = (cfg.games_dir.clone(), cfg.data_dir.clone());
    let mut st = AppState::new(cfg);
    st.uploads.free = free;
    st.scan_gap = std::time::Duration::ZERO;
    let state = Arc::new(st);
    Rig {
        _t: t,
        app: router(state.clone()),
        games,
        data,
        state,
    }
}

fn disc(id: &[u8; 6], wii: bool, title: &str) -> Vec<u8> {
    let mut h = vec![0u8; 300];
    h[..6].copy_from_slice(id);
    let (at, m): (usize, [u8; 4]) = if wii {
        (0x18, [0x5D, 0x1C, 0x9E, 0xA3])
    } else {
        (0x1C, [0xC2, 0x33, 0x9F, 0x3D])
    };
    h[at..at + 4].copy_from_slice(&m);
    h[0x20..0x20 + title.len()].copy_from_slice(title.as_bytes());
    h
}

impl Rig {
    async fn req(
        &self,
        method: &str,
        uri: &str,
        hdr: &[(&str, String)],
        body: Body,
        auth: bool,
    ) -> (StatusCode, Value) {
        let mut r = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "localhost:8443");
        if auth {
            r = r.header("x-brisk-secret", SECRET);
        }
        for (k, v) in hdr {
            r = r.header(*k, v);
        }
        let r = self
            .app
            .clone()
            .oneshot(r.body(body).unwrap())
            .await
            .unwrap();
        let st = r.status();
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        (st, serde_json::from_slice(&b).unwrap_or_default())
    }
    async fn call(&self, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let b = if body.is_null() {
            Body::empty()
        } else {
            Body::from(body.to_string())
        };
        self.req(method, uri, &[], b, true).await
    }
    async fn create(&self, name: &str, size: usize) -> (StatusCode, Value) {
        self.call("POST", "/api/uploads", json!({"name": name, "size": size}))
            .await
    }
    async fn chunk(&self, id: &str, off: usize, bytes: &[u8]) -> (StatusCode, Value) {
        self.req(
            "PUT",
            &format!("/api/uploads/{id}"),
            &[("x-offset", off.to_string())],
            Body::from(bytes.to_vec()),
            true,
        )
        .await
    }
    /// An upload of `bytes` named `name`, sent in `step`-byte chunks, not finished.
    async fn upload(&self, name: &str, bytes: &[u8], step: usize) -> String {
        let (st, v) = self.create(name, bytes.len()).await;
        assert_eq!(st, StatusCode::CREATED, "{v}");
        let id = v["id"].as_str().unwrap().to_string();
        for (i, c) in bytes.chunks(step).enumerate() {
            assert_eq!(self.chunk(&id, i * step, c).await.0, StatusCode::OK);
        }
        id
    }
}

#[tokio::test]
async fn upload_in_chunks_lands_in_the_system_folder_and_gets_a_card() {
    let r = rig();
    let bytes = disc(b"RMCE01", true, "Mario Kart Wii");
    let (st, v) = r.create("Mario Kart.iso", bytes.len()).await;
    assert_eq!(
        (st, v["chunk"].as_u64(), v["offset"].as_u64()),
        (StatusCode::CREATED, Some(32 << 20), Some(0))
    );
    let id = v["id"].as_str().unwrap();
    assert_eq!(id.len(), 32);
    let part = r.games.join(format!(".uploads/{id}.part"));
    assert!(part.is_file());
    for (i, c) in bytes.chunks(100).enumerate() {
        let (st, v) = r.chunk(id, i * 100, c).await;
        assert_eq!(
            (st, v["offset"].as_u64()),
            (StatusCode::OK, Some((i * 100 + c.len()) as u64))
        );
    }
    let (_, s) = r
        .call("GET", &format!("/api/uploads/{id}"), Value::Null)
        .await;
    assert_eq!(
        (
            s["offset"].as_u64(),
            s["size"].as_u64(),
            s["state"].as_str()
        ),
        (Some(300), Some(300), Some("open"))
    );
    let (st, f) = r
        .call("POST", &format!("/api/uploads/{id}/finish"), Value::Null)
        .await;
    assert_eq!(st, StatusCode::OK, "{f}");
    assert_eq!(
        (
            f["system"].as_str(),
            f["registered"].as_bool(),
            f["duplicate"].as_bool()
        ),
        (Some("wii"), Some(true), Some(false))
    );
    assert_eq!(
        (f["game"]["id"].as_str(), f["game"]["name"].as_str()),
        (Some("emu:dolphin:RMCE01"), Some("Mario Kart Wii"))
    );
    let dest = r.games.join("wii/Mario Kart.iso");
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert!(!part.exists());
    // unix mode of the part file was 0600; the finished file is an ordinary file
    let (_, g) = r.call("GET", "/api/games", Value::Null).await;
    assert!(
        g["games"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == "emu:dolphin:RMCE01")
    );
    assert_eq!(
        r.call("GET", &format!("/api/uploads/{id}"), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // the same name again gets a suffix and the same disc is a duplicate card
    let id2 = r.upload("Mario Kart.iso", &bytes, 300).await;
    let (_, f2) = r
        .call("POST", &format!("/api/uploads/{id2}/finish"), Value::Null)
        .await;
    assert_eq!(
        (f2["duplicate"].as_bool(), f2["game"]["id"].as_str()),
        (Some(true), Some("emu:dolphin:RMCE01"))
    );
    assert!(r.games.join("wii/Mario Kart (1).iso").is_file());
}

#[tokio::test]
async fn part_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let r = rig();
    let (_, v) = r.create("a.iso", 10).await;
    let p = r
        .games
        .join(format!(".uploads/{}.part", v["id"].as_str().unwrap()));
    assert_eq!(
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(r.games.join(".uploads"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[tokio::test]
async fn wrong_offset_is_409_with_the_current_one() {
    let r = rig();
    let (_, v) = r.create("a.iso", 300).await;
    let id = v["id"].as_str().unwrap();
    r.chunk(id, 0, &[1u8; 100]).await;
    for off in [0, 50, 200] {
        let (st, v) = r.chunk(id, off, &[2u8; 10]).await;
        assert_eq!(
            (st, v["offset"].as_u64()),
            (StatusCode::CONFLICT, Some(100))
        );
    }
    // no header, a bad header, an empty chunk, a chunk past the size
    assert_eq!(
        r.req(
            "PUT",
            &format!("/api/uploads/{id}"),
            &[],
            Body::from("x"),
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let bad = r
        .req(
            "PUT",
            &format!("/api/uploads/{id}"),
            &[("x-offset", "-1".into())],
            Body::from("x"),
            true,
        )
        .await;
    assert_eq!(bad.0, StatusCode::BAD_REQUEST);
    assert_eq!(r.chunk(id, 100, b"").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        r.chunk(id, 100, &[0u8; 201]).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        r.call("GET", &format!("/api/uploads/{id}"), Value::Null)
            .await
            .1["offset"],
        100
    );
}

#[tokio::test]
async fn dropped_connection_then_resume() {
    let r = rig();
    let (_, v) = r.create("a.iso", 300).await;
    let id = v["id"].as_str().unwrap();
    r.chunk(id, 0, &[1u8; 100]).await;
    // 60 bytes arrive, then the connection dies
    let s = futures_util::stream::iter(vec![
        Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![9u8; 60])),
        Err(std::io::Error::other("reset")),
    ]);
    let (st, _) = r
        .req(
            "PUT",
            &format!("/api/uploads/{id}"),
            &[("x-offset", "100".into())],
            Body::from_stream(s),
            true,
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (_, s) = r
        .call("GET", &format!("/api/uploads/{id}"), Value::Null)
        .await;
    assert_eq!(
        (s["offset"].as_u64(), s["state"].as_str()),
        (Some(100), Some("open"))
    );
    // the client asks, then sends from the offset; the stray 60 bytes are gone
    assert_eq!(r.chunk(id, 100, &[2u8; 200]).await.1["offset"], 300);
    let part = std::fs::read(r.games.join(format!(".uploads/{id}.part"))).unwrap();
    assert_eq!(part.len(), 300);
    assert!(part[..100].iter().all(|&b| b == 1) && part[100..].iter().all(|&b| b == 2));
}

#[tokio::test]
async fn finish_checks_the_size_and_the_header() {
    let r = rig();
    let (_, v) = r.create("a.iso", 300).await;
    let id = v["id"].as_str().unwrap();
    r.chunk(id, 0, &[0u8; 100]).await;
    let (st, v) = r
        .call("POST", &format!("/api/uploads/{id}/finish"), Value::Null)
        .await;
    assert_eq!(
        (st, v["offset"].as_u64(), v["size"].as_u64()),
        (StatusCode::CONFLICT, Some(100), Some(300))
    );
    // still open: finish the rest. Zeros are no disc: stored under unknown/, no card
    r.chunk(id, 100, &[0u8; 200]).await;
    let (st, v) = r
        .call("POST", &format!("/api/uploads/{id}/finish"), Value::Null)
        .await;
    assert_eq!(
        (st, v["system"].as_str(), v["registered"].as_bool()),
        (StatusCode::OK, Some("unknown"), Some(false))
    );
    assert!(r.games.join("unknown/a.iso").is_file());
    assert!(!r.data.join("library.json").exists());

    // a known format with a broken header is refused and the partial file is gone
    let mut w = b"WBFS".to_vec();
    w.resize(20, 0);
    let id = r.upload("b.wbfs", &w, 20).await;
    let (st, _) = r
        .call("POST", &format!("/api/uploads/{id}/finish"), Value::Null)
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        std::fs::read_dir(r.games.join(".uploads")).unwrap().count(),
        0
    );
    assert!(!r.games.join("wbfs").exists() && !r.games.join("unknown/b.wbfs").exists());
}

#[tokio::test]
async fn cartridges_are_stored_but_have_no_card() {
    let r = rig();
    let mut g = vec![0u8; 0x200];
    g[4..8].copy_from_slice(&[0x24, 0xFF, 0xAE, 0x51]);
    g[0xB2] = 0x96;
    let id = r.upload("x.gba", &g, 512).await;
    let (_, f) = r
        .call("POST", &format!("/api/uploads/{id}/finish"), Value::Null)
        .await;
    assert_eq!(
        (f["system"].as_str(), f["registered"].as_bool()),
        (Some("gba"), Some(false))
    );
    assert!(r.games.join("gba/x.gba").is_file());
}

#[tokio::test]
async fn free_space_refusals_keep_5_gb() {
    let r = rig();
    FREE.with(|f| f.set(10 * GB));
    assert_eq!(
        r.create("a.iso", (5 * GB) as usize).await.0,
        StatusCode::CREATED
    ); // 5 GB left after: ok
    FREE.with(|f| f.set(10 * GB - 1));
    assert_eq!(
        r.create("b.iso", (5 * GB) as usize).await.0,
        StatusCode::INSUFFICIENT_STORAGE
    );
    // no cap of its own: a 20 GB file is fine on a big disk
    FREE.with(|f| f.set(100 * GB));
    let (st, v) = r.create("c.iso", (20 * GB) as usize).await;
    assert_eq!(st, StatusCode::CREATED);
    // the disk fills while the upload is open: the next chunk is refused, the upload survives
    FREE.with(|f| f.set(20 * GB));
    let id = v["id"].as_str().unwrap();
    let (st, e) = r.chunk(id, 0, &[0u8; 10]).await;
    assert_eq!(st, StatusCode::INSUFFICIENT_STORAGE);
    assert!(e["error"].as_str().unwrap().contains("5 GB"));
    FREE.with(|f| f.set(100 * GB));
    assert_eq!(r.chunk(id, 0, &[0u8; 10]).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_full_size_chunk_goes_through() {
    let r = rig();
    let (_, v) = r.create("big.iso", 40 << 20).await;
    let id = v["id"].as_str().unwrap();
    assert_eq!(
        r.chunk(id, 0, &vec![0u8; 32 << 20]).await.1["offset"],
        32 << 20
    );
    assert_eq!(
        r.chunk(id, 32 << 20, &vec![0u8; 8 << 20]).await.1["offset"],
        40 << 20
    );
}

#[tokio::test]
async fn chunk_size_is_capped_without_buffering_more() {
    let r = rig();
    let (_, v) = r.create("big.iso", 100 << 20).await;
    let id = v["id"].as_str().unwrap();
    let over = (32 << 20) + 1;
    // by Content-Length, before reading anything
    let h = [
        ("x-offset", "0".to_string()),
        ("content-length", over.to_string()),
    ];
    assert_eq!(
        r.req(
            "PUT",
            &format!("/api/uploads/{id}"),
            &h,
            Body::empty(),
            true
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    // and by counting a body that has no length
    let s = futures_util::stream::iter(
        (0..33).map(|_| Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![0u8; 1 << 20]))),
    );
    let st = r
        .req(
            "PUT",
            &format!("/api/uploads/{id}"),
            &[("x-offset", "0".into())],
            Body::from_stream(s),
            true,
        )
        .await
        .0;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        r.call("GET", &format!("/api/uploads/{id}"), Value::Null)
            .await
            .1["offset"],
        0
    );
}

#[tokio::test]
async fn two_uploads_at_a_time_and_abort() {
    let r = rig();
    let a = r.create("a.iso", 10).await.1["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b = r.create("b.iso", 10).await.1["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(r.create("c.iso", 10).await.0, StatusCode::TOO_MANY_REQUESTS);
    let part = r.games.join(format!(".uploads/{a}.part"));
    assert!(part.exists());
    assert_eq!(
        r.call("DELETE", &format!("/api/uploads/{a}"), Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(!part.exists());
    assert_eq!(
        r.call("DELETE", &format!("/api/uploads/{a}"), Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        r.call("GET", &format!("/api/uploads/{a}"), Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(r.chunk(&a, 0, b"x").await.0, StatusCode::NOT_FOUND);
    assert_eq!(r.create("c.iso", 10).await.0, StatusCode::CREATED);
    assert_eq!(
        r.call("GET", &format!("/api/uploads/{b}"), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn hostile_names_and_ids() {
    let r = rig();
    for n in [
        "../x.iso",
        "a/b.iso",
        "a\\b.iso",
        "/etc/passwd",
        "x\0.iso",
        "..",
        "",
        ".  ",
    ] {
        let (st, _) = r.create(n, 10).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{n:?}");
    }
    assert_eq!(
        r.create("run.exe", 10).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        r.create("noext", 10).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(r.create("a.iso", 0).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        r.call("POST", "/api/uploads", json!({"name": "a.iso"}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // control characters are stripped by sanitize, the name stays inside the folder
    let (st, v) = r.create("we\u{202e}ird\n.iso", 10).await;
    assert_eq!(st, StatusCode::CREATED);
    let (_, s) = r
        .call(
            "GET",
            &format!("/api/uploads/{}", v["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    assert_eq!(s["name"], "weird.iso");
    for id in ["..", "zz", "%2e%2e%2f", "0".repeat(32).as_str()] {
        assert_eq!(
            r.call("GET", &format!("/api/uploads/{id}"), Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND,
            "{id}"
        );
    }
}

#[tokio::test]
async fn old_partials_are_cleaned_at_startup() {
    let r = rig();
    let dir = r.games.join(".uploads");
    std::fs::create_dir_all(&dir).unwrap();
    let (old, new) = (dir.join("old.part"), dir.join("new.part"));
    std::fs::write(&old, b"x").unwrap();
    std::fs::write(&new, b"x").unwrap();
    let two_days = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(two_days)
        .unwrap();
    r.state.uploads.clean_old();
    assert!(!old.exists() && new.exists());
}

#[tokio::test]
async fn every_route_needs_auth() {
    let r = rig();
    let id = r.create("a.iso", 10).await.1["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (m, u) in [
        ("POST", "/api/uploads".to_string()),
        ("GET", format!("/api/uploads/{id}")),
        ("PUT", format!("/api/uploads/{id}")),
        ("POST", format!("/api/uploads/{id}/finish")),
        ("DELETE", format!("/api/uploads/{id}")),
        ("POST", "/api/library/scan".to_string()),
        ("DELETE", "/api/games/emu:dolphin:X".to_string()),
    ] {
        let (st, _) = r
            .req(m, &u, &[("x-offset", "0".into())], Body::empty(), false)
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m} {u}");
    }
    assert_eq!(
        r.call("GET", &format!("/api/uploads/{id}"), Value::Null)
            .await
            .1["offset"],
        0
    );
}

fn write(p: PathBuf, b: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b).unwrap();
}

#[tokio::test]
async fn scan_route_adds_once_and_is_rate_limited() {
    let r = rig();
    write(
        r.games.join("wii/a.iso"),
        &disc(b"RMCE01", true, "Mario Kart Wii"),
    );
    write(
        r.games.join("scp/drop.iso"),
        &disc(b"GALE01", false, "Melee"),
    );
    write(r.games.join("gba/g.gba"), &{
        let mut g = vec![0u8; 0x200];
        g[4..8].copy_from_slice(&[0x24, 0xFF, 0xAE, 0x51]);
        g[0xB2] = 0x96;
        g
    });
    write(r.games.join("wii/readme.iso"), b"nope");
    let (st, v) = r.call("POST", "/api/library/scan", Value::Null).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["added"].as_array().unwrap().len(),
            v["unplayable"].as_u64(),
            v["unknown"].as_u64(),
            v["scanned"].as_u64()
        ),
        (2, Some(1), Some(1), Some(4))
    );
    let ids: Vec<_> = v["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"emu:dolphin:RMCE01") && ids.contains(&"emu:dolphin:GALE01"));
    let (_, v) = r.call("POST", "/api/library/scan", Value::Null).await;
    assert_eq!(
        (
            v["added"].as_array().unwrap().len(),
            v["duplicates"].as_u64()
        ),
        (0, Some(2))
    );

    // with a gap configured, a second scan right away waits
    let mut st = AppState::new(r.state.cfg.clone());
    st.scan_gap = std::time::Duration::from_secs(60);
    let app = router(Arc::new(st));
    let go = || {
        app.clone().oneshot(
            Request::post("/api/library/scan")
                .header("host", "localhost:8443")
                .header("x-brisk-secret", SECRET)
                .body(Body::empty())
                .unwrap(),
        )
    };
    assert_eq!(go().await.unwrap().status(), StatusCode::OK);
    let second = go().await.unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(second.headers().contains_key("retry-after"));
}

#[tokio::test]
async fn scan_while_running_is_409_and_the_slot_is_freed() {
    // Hold the library lock from here so the scan blocks inside its merge step.
    let r = rig();
    write(r.games.join("wii/a.iso"), &disc(b"RMCE01", true, "M"));
    let lock = brisk_hostd::ingest::lock(&r.data).unwrap();
    let first = tokio::spawn({
        let (app,) = (r.app.clone(),);
        async move {
            app.oneshot(
                Request::post("/api/library/scan")
                    .header("host", "localhost:8443")
                    .header("x-brisk-secret", SECRET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        r.call("POST", "/api/library/scan", Value::Null).await.0,
        StatusCode::CONFLICT
    );
    drop(lock);
    assert_eq!(first.await.unwrap(), StatusCode::OK);
    assert_eq!(
        r.call("POST", "/api/library/scan", Value::Null).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn remove_forgets_the_record_and_keeps_the_file() {
    let r = rig();
    let f = r.games.join("wii/a.iso");
    write(f.clone(), &disc(b"RMCE01", true, "Mario Kart Wii"));
    r.call("POST", "/api/library/scan", Value::Null).await;
    std::fs::write(r.data.join("notes.json"), "{}").unwrap();
    let u = "/api/games/emu:dolphin:RMCE01";
    assert_eq!(
        r.call("DELETE", u, Value::Null).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        r.call("DELETE", u, Value::Null).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        r.call("DELETE", "/api/games/emu%3Adolphin%3ANOPE", Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(f.is_file() && r.data.join("notes.json").is_file());
    let (_, g) = r.call("GET", "/api/games", Value::Null).await;
    assert!(
        !g["games"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == "emu:dolphin:RMCE01")
    );
    let (st, v) = r.call("DELETE", "/api/games/steam:620", Value::Null).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("not supported"));
    assert_eq!(
        r.call("DELETE", "/api/games/banana", Value::Null).await.0,
        StatusCode::BAD_REQUEST
    );
    // a plain scan leaves it out; asking for removed games brings it back, under the same id
    let (_, v) = r
        .call("POST", "/api/library/scan?include_removed=1", Value::Null)
        .await;
    assert_eq!(v["added"][0]["id"], "emu:dolphin:RMCE01");
}

#[tokio::test]
async fn a_removed_game_stays_gone_until_added_on_purpose() {
    let r = rig();
    let f = r.games.join("wii/a.iso");
    write(f.clone(), &disc(b"RMCE01", true, "Mario Kart Wii"));
    r.call("POST", "/api/library/scan", Value::Null).await;
    r.call("DELETE", "/api/games/emu:dolphin:RMCE01", Value::Null)
        .await;
    assert!(r.data.join("removed.json").is_file());
    // a plain scan skips it and counts it
    let (_, v) = r.call("POST", "/api/library/scan", Value::Null).await;
    assert_eq!(
        (v["added"].as_array().unwrap().len(), v["removed"].as_u64()),
        (0, Some(1))
    );
    // include_removed ignores the list, and the game is back for good
    let (_, v) = r
        .call("POST", "/api/library/scan?include_removed=1", Value::Null)
        .await;
    assert_eq!(v["added"][0]["id"], "emu:dolphin:RMCE01");
    let (_, v) = r.call("POST", "/api/library/scan", Value::Null).await;
    assert_eq!(
        (v["removed"].as_u64(), v["duplicates"].as_u64()),
        (Some(0), Some(1))
    );
    // remove again, then upload the same disc: an explicit add un-removes it
    r.call("DELETE", "/api/games/emu:dolphin:RMCE01", Value::Null)
        .await;
    let id = r
        .upload("b.iso", &disc(b"RMCE01", true, "Mario Kart Wii"), 1 << 20)
        .await;
    let (st, v) = r
        .call("POST", &format!("/api/uploads/{id}/finish"), Value::Null)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["registered"], true);
    assert_eq!(v["game"]["id"], "emu:dolphin:RMCE01");
    let (_, v) = r.call("POST", "/api/library/scan", Value::Null).await;
    assert_eq!(v["removed"].as_u64(), Some(0));
}
