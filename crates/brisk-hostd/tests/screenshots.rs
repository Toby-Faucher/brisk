mod common;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use brisk_hostd::http::{AppState, router};
use common::*;
use std::sync::Arc;
use tower::ServiceExt;

const LIB: &str = r#"{"v":1,"games":[
 {"id":"emu:dolphin:rmce01","emulator":"dolphin","system":"wii","file":"/x/a.iso","name":"Wii game"}]}"#;
const G: &str = "/api/games/emu:dolphin:rmce01/screenshots";

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    v.extend(13u32.to_be_bytes());
    v.extend(b"IHDR");
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
    v.extend(b"\0\0\0\0IEND\xaeB`\x82");
    v
}

fn setup() -> Steam {
    let s = steam("http://127.0.0.1:1", "http://127.0.0.1:1", None);
    std::fs::create_dir_all(&s.cfg.data_dir).unwrap();
    std::fs::write(s.cfg.data_dir.join("library.json"), LIB).unwrap();
    s
}

async fn call(
    s: &Steam,
    m: Method,
    uri: &str,
    body: Body,
    secret: bool,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut r = Request::builder().method(m).uri(uri).header("host", H);
    if secret {
        r = r.header("x-brisk-secret", SECRET);
    }
    let r = router(Arc::new(AppState::new(s.cfg.clone())))
        .oneshot(r.body(body).unwrap())
        .await
        .unwrap();
    let (st, h) = (r.status(), r.headers().clone());
    (
        st,
        h,
        axum::body::to_bytes(r.into_body(), 1 << 24)
            .await
            .unwrap()
            .to_vec(),
    )
}
async fn post(s: &Steam, b: Vec<u8>) -> (StatusCode, serde_json::Value) {
    let (st, _, b) = call(s, Method::POST, G, Body::from(b), true).await;
    (st, serde_json::from_slice(&b).unwrap_or_default())
}

#[tokio::test]
async fn needs_the_secret_on_every_route() {
    let s = setup();
    for (m, u) in [
        (Method::POST, G),
        (Method::GET, G),
        (Method::GET, &format!("{G}/1730000000000.png")),
        (Method::DELETE, &format!("{G}/1730000000000.png")),
    ] {
        let (st, ..) = call(&s, m, u, Body::from(png(2, 2)), false).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{u}");
    }
}

#[tokio::test]
async fn upload_list_serve_delete() {
    let s = setup();
    let (st, v) = post(&s, png(640, 480)).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(
        (v["width"].clone(), v["height"].clone()),
        (640.into(), 480.into())
    );
    let name = v["name"].as_str().unwrap().to_string();
    tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    let (_, v2) = post(&s, png(10, 20)).await;
    let (_, _, b) = call(&s, Method::GET, G, Body::empty(), true).await;
    let l: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(l[0]["name"], v2["name"], "newest first");
    assert_eq!(l[1]["name"].as_str().unwrap(), name);
    assert!(l[0]["takenAt"].as_u64().unwrap() > l[1]["takenAt"].as_u64().unwrap());
    // the game's own screenshot listing (what the panel reads) and the file
    let (st, loc) = get_json(&s.cfg, "/api/games/emu:dolphin:rmce01/local").await;
    assert_eq!(
        (st, loc["screenshots"][1].as_str().unwrap()),
        (StatusCode::OK, name.as_str())
    );
    let (st, h, body) = call(&s, Method::GET, &format!("{G}/{name}"), Body::empty(), true).await;
    assert_eq!((st, body), (StatusCode::OK, png(640, 480)));
    assert_eq!(h["content-type"], "image/png");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(h["cache-control"].to_str().unwrap().starts_with("private"));
    // files are private
    let f = s.cfg.data_dir.join("shots/emu_dolphin_rmce01").join(&name);
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(f).unwrap().permissions())
            & 0o777,
        0o600
    );
    // delete twice: both 204, then 404 on read
    for _ in 0..2 {
        let (st, ..) = call(
            &s,
            Method::DELETE,
            &format!("{G}/{name}"),
            Body::empty(),
            true,
        )
        .await;
        assert_eq!(st, StatusCode::NO_CONTENT);
    }
    let (st, ..) = call(&s, Method::GET, &format!("{G}/{name}"), Body::empty(), true).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_bodies_are_refused() {
    let s = setup();
    let mut cut = png(10, 10);
    cut.truncate(cut.len() - 4);
    let mut bad_magic = png(10, 10);
    bad_magic[1] = b'X';
    for (b, why) in [
        (bad_magic, "magic"),
        (png(8193, 1), "wide"),
        (png(1, 9000), "tall"),
        (cut, "cut"),
        (b"hello".to_vec(), "text"),
        (vec![], "empty"),
    ] {
        let (st, _) = post(&s, b).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{why}");
    }
    assert!(
        !s.cfg.data_dir.join("shots").exists()
            || std::fs::read_dir(s.cfg.data_dir.join("shots/emu_dolphin_rmce01"))
                .unwrap()
                .count()
                == 0
    );
    // over 8 MiB: refused while it streams (no content-length)
    let big =
        futures_util::stream::iter((0..9).map(|_| Ok::<_, std::io::Error>(vec![0u8; 1 << 20])));
    let (st, ..) = call(&s, Method::POST, G, Body::from_stream(big), true).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
    let (st, ..) = call(&s, Method::POST, G, Body::from(vec![0u8; 9 << 20]), true).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn names_and_games_are_checked() {
    let s = setup();
    let (_, v) = post(&s, png(4, 4)).await;
    let name = v["name"].as_str().unwrap();
    for n in [
        "..%2F..%2Flibrary.json",
        "..",
        "%2e%2e%2f1730000000000.png",
        "1730000000000.png%00",
        "a.png",
        &format!("{name}%2f.."),
        "1730000000000.PNG",
    ] {
        for m in [Method::GET, Method::DELETE] {
            let (st, ..) = call(&s, m.clone(), &format!("{G}/{n}"), Body::empty(), true).await;
            assert!(
                st == StatusCode::NOT_FOUND
                    || st == StatusCode::BAD_REQUEST
                    || st == StatusCode::METHOD_NOT_ALLOWED,
                "{m} {n}: {st}"
            );
        }
    }
    assert!(s.cfg.data_dir.join("library.json").exists());
    // Steam and unknown games have no stored shots; the old numeric form is not an emu id
    for u in ["steam:620", "emu:dolphin:nope", "620"] {
        let (st, ..) = call(
            &s,
            Method::POST,
            &format!("/api/games/{u}/screenshots"),
            Body::from(png(2, 2)),
            true,
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{u}");
    }
    // the cap shows on the game
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    let g = v["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "emu:dolphin:rmce01")
        .unwrap();
    assert_eq!(g["caps"]["screenshots"], true);
}
