mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::http::{AppState, router};
use common::*;
use std::sync::Arc;
use tower::ServiceExt;

const LIB: &str = r#"{"v":1,"games":[
 {"id":"emu:dolphin:rmce01","emulator":"dolphin","system":"wii","file":"/x/a.iso","name":"Wii game"},
 {"id":"emu:dolphin:gale01","emulator":"dolphin","system":"gamecube","file":"/x/b.iso","name":"GC game"},
 {"id":"emu:dolphin:no-header","emulator":"dolphin","system":"wii","file":"/x/c.iso","name":"Slug id"},
 {"id":"emu:dolphin:nosystem","emulator":"dolphin","file":"/x/d.iso","name":"No system"}]}"#;

fn setup_dir(with: bool) -> Steam {
    let mut s = steam("http://127.0.0.1:1", "http://127.0.0.1:1", None);
    s.cfg.dolphin_user_dir = s._t.path().join("dolphin-user");
    std::fs::create_dir_all(&s.cfg.data_dir).unwrap();
    std::fs::write(s.cfg.data_dir.join("library.json"), LIB).unwrap();
    if with {
        let u = s.cfg.dolphin_user_dir.clone();
        std::fs::create_dir_all(u.join("StateSaves")).unwrap();
        std::fs::write(u.join("StateSaves/RMCE01.s03"), b"12345").unwrap();
        let d = u.join("Wii/title/00010000/524d4345/data");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a"), b"12").unwrap();
        std::fs::create_dir_all(u.join("GC")).unwrap();
        std::fs::write(u.join("GC/MemoryCardA.USA.raw"), b"x").unwrap();
    }
    s
}

#[tokio::test]
async fn saves_route_lists_items_and_hides_the_dir() {
    let s = setup_dir(true);
    let (st, v) = get_json(&s.cfg, "/api/games/emu:dolphin:rmce01/saves").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["available"], true);
    assert_eq!(v["userDir"], "Dolphin user folder");
    let kinds: Vec<_> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"state") && kinds.contains(&"nand-save"),
        "{v}"
    );
    assert!(!kinds.contains(&"memory-card"));
    assert!(
        !v.to_string().contains(s._t.path().to_str().unwrap()),
        "no absolute path"
    );
    let (_, g) = get_json(&s.cfg, "/api/games/emu:dolphin:gale01/saves").await;
    assert_eq!(g["items"][0]["kind"], "memory-card");
    assert!(g["items"][0]["note"].is_string());
}

#[tokio::test]
async fn saves_route_edges() {
    let s = setup_dir(false);
    let (st, v) = get_json(&s.cfg, "/api/games/emu:dolphin:rmce01/saves").await;
    assert_eq!(
        (st, v["available"].clone(), v["items"].clone()),
        (StatusCode::OK, false.into(), serde_json::json!([]))
    );
    // a slug id has no disc id: nothing to look for, still a 200
    let s = setup_dir(true);
    let (st, v) = get_json(&s.cfg, "/api/games/emu:dolphin:no-header/saves").await;
    assert_eq!(
        (st, v["items"].as_array().unwrap().len()),
        (StatusCode::OK, 0)
    );
    // no system: no saves cap, 404; unknown game 404; no secret 403
    for u in ["emu:dolphin:nosystem", "emu:dolphin:nope", "steam:620"] {
        let (st, _) = get_json(&s.cfg, &format!("/api/games/{u}/saves")).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{u}");
    }
    let r = router(Arc::new(AppState::new(s.cfg.clone())))
        .oneshot(
            Request::builder()
                .uri("/api/games/emu:dolphin:rmce01/saves")
                .header("host", H)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn no_store_and_caps_by_system() {
    let s = setup_dir(true);
    let r = router(Arc::new(AppState::new(s.cfg.clone())))
        .oneshot(
            Request::builder()
                .uri("/api/games/emu:dolphin:rmce01/saves")
                .header("host", H)
                .header("x-brisk-secret", SECRET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.headers()["cache-control"], "no-store");
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    let saves = |id: &str| {
        v["games"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == id)
            .unwrap()["caps"]["saves"]
            .clone()
    };
    assert_eq!(saves("emu:dolphin:rmce01"), true);
    assert_eq!(saves("emu:dolphin:gale01"), true);
    assert_eq!(saves("emu:dolphin:nosystem"), false);
}
