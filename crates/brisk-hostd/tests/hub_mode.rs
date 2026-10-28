//! A hub (machines.json loaded) has no library of its own: machine-level routes answer 409.
mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::http::{AppState, router};
use brisk_hostd::machines::{Hub, parse};
use common::{H, SECRET, steam};
use std::sync::Arc;
use tower::ServiceExt;

fn app(hub: bool) -> (axum::Router, common::Steam) {
    let s = steam("http://127.0.0.1:1", "http://127.0.0.1:1", None);
    let mut st = AppState::new(s.cfg.clone());
    if hub {
        let cfgs = parse(
            r#"{"machines":[{"id":"vm1","name":"VM","url":"http://127.0.0.1:1","secret_file":"/nonexistent"}]}"#,
        )
        .unwrap();
        st.hub = Some(Arc::new(Hub::new(cfgs, |_, _| unreachable!())));
    }
    (router(Arc::new(st)), s)
}

async fn send(app: &axum::Router, m: &str, uri: &str, auth: bool) -> (StatusCode, String) {
    let mut r = Request::builder().method(m).uri(uri).header("host", H);
    if auth {
        r = r.header("x-brisk-secret", SECRET);
    }
    let r = app
        .clone()
        .oneshot(r.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let st = r.status();
    let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    (st, String::from_utf8_lossy(&b).into())
}

const MACHINE_LEVEL: &[(&str, &str)] = &[
    ("GET", "/api/games"),
    ("GET", "/api/games/1/info"),
    ("DELETE", "/api/games/1"),
    ("GET", "/api/friends"),
    ("GET", "/api/launch/defaults"),
    ("POST", "/api/library/scan"),
    ("POST", "/api/uploads"),
    ("GET", "/api/uploads/x"),
    ("GET", "/api/art/pending"),
    ("GET", "/api/emulators/dolphin/controls/gc"),
];

#[tokio::test]
async fn hub_answers_409_on_machine_routes() {
    let (a, _s) = app(true);
    for (m, u) in MACHINE_LEVEL {
        let (st, body) = send(&a, m, u, true).await;
        assert_eq!(st, StatusCode::CONFLICT, "{m} {u}");
        assert_eq!(body, r#"{"error":"pick a machine"}"#, "{m} {u}");
        assert_eq!(
            send(&a, m, u, false).await.0,
            StatusCode::FORBIDDEN,
            "{m} {u}"
        );
    }
}

#[tokio::test]
async fn hub_keeps_its_own_routes() {
    let (a, _s) = app(true);
    for u in ["/api/machines", "/api/prefs", "/api/notes"] {
        assert_eq!(send(&a, "GET", u, true).await.0, StatusCode::OK, "{u}");
    }
    // proxied, not 409 (the machine is unreachable here)
    let (st, _) = send(&a, "GET", "/api/m/vm1/api/games", true).await;
    assert_ne!(st, StatusCode::CONFLICT);
    assert_eq!(
        send(&a, "GET", "/api/m/vm1/api/games", false).await.0,
        StatusCode::FORBIDDEN
    );
    // remap is the hub's own
    let (st, _) = send(&a, "POST", "/api/ids/remap", true).await;
    assert_ne!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn standalone_unchanged() {
    let (a, _s) = app(false);
    assert_eq!(send(&a, "GET", "/api/games", true).await.0, StatusCode::OK);
    assert_eq!(
        send(&a, "GET", "/api/launch/defaults", true).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&a, "GET", "/api/friends", true).await.0,
        StatusCode::OK
    );
}
