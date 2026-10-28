use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use tower::ServiceExt;

const SECRET: &str = "s3cret";

struct T {
    app: axum::Router,
    dir: tempfile::TempDir,
}

fn t() -> T {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env(dir.path().to_path_buf());
    cfg.secret = SECRET.into();
    cfg.data_dir = dir.path().to_path_buf();
    T {
        app: router(Arc::new(AppState::new(cfg))),
        dir,
    }
}

impl T {
    async fn call(
        &self,
        m: &str,
        u: &str,
        secret: bool,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut r = Request::builder()
            .method(m)
            .uri(u)
            .header("host", "localhost:8443");
        if secret {
            r = r.header("x-brisk-secret", SECRET);
        }
        let b = body.map_or(Body::empty(), |v| Body::from(v.to_string()));
        let res = self.app.clone().oneshot(r.body(b).unwrap()).await.unwrap();
        let st = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (st, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
    async fn c(&self, m: &str, u: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call(m, u, true, body).await
    }
}

fn defaults(allow: bool, options: Value) -> Value {
    json!({ "allowCustomCommands": allow, "options": options })
}

#[tokio::test]
async fn every_route_needs_auth() {
    let t = t();
    for (m, u) in [
        ("GET", "/api/launch/defaults"),
        ("PUT", "/api/launch/defaults"),
        ("GET", "/api/games/steam:620/launch"),
        ("PUT", "/api/games/steam:620/launch"),
        ("DELETE", "/api/games/steam:620/launch"),
    ] {
        let (st, _) = t.call(m, u, false, Some(json!({}))).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m} {u}");
    }
    assert!(!t.dir.path().join("launch.json").exists());
}

#[tokio::test]
async fn defaults_round_trip_and_file_is_private() {
    let t = t();
    let (st, v) = t.c("GET", "/api/launch/defaults", None).await;
    assert_eq!(
        (st, v["allowCustomCommands"].clone()),
        (StatusCode::OK, json!(false))
    );
    assert!(
        v["envAllowed"]
            .as_array()
            .unwrap()
            .contains(&json!("DXVK_HUD"))
    );
    let o = json!({"wrappers": ["mangohud"], "env": {"DXVK_HUD": "fps"}});
    let (st, v) = t
        .c(
            "PUT",
            "/api/launch/defaults",
            Some(defaults(false, o.clone())),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["options"]["wrappers"], json!(["mangohud"]));
    let f = t.dir.path().join("launch.json");
    assert_eq!(
        std::fs::metadata(f).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let (_, v) = t.c("GET", "/api/launch/defaults", None).await;
    assert_eq!(v["options"]["env"]["DXVK_HUD"], "fps");
}

#[tokio::test]
async fn bad_options_are_400_with_plain_problems_and_nothing_is_saved() {
    let t = t();
    let cases = [
        (
            defaults(false, json!({"env": {"LD_PRELOAD": "/x.so"}})),
            "LD_PRELOAD",
        ),
        (
            defaults(false, json!({"env": {"A=b;rm": "1"}})),
            "allowed list",
        ),
        (
            defaults(false, json!({"env": {"DXVK_HUD": "a\nb"}})),
            "line break",
        ),
        (
            defaults(
                false,
                json!({"wrappers": [{"gamescope": {"args": ["--exec", "sh"]}}]}),
            ),
            "gamescope",
        ),
        (
            defaults(false, json!({"wrappers": ["mangohud", "mangohud"]})),
            "twice",
        ),
        (defaults(false, json!({"custom": ["tool"]})), "switched off"),
        (
            defaults(true, json!({"custom": ["../bin/sh"]})),
            "custom command",
        ),
        (defaults(true, json!({"custom": []})), "custom command"),
    ];
    for (body, word) in cases {
        let (st, v) = t.c("PUT", "/api/launch/defaults", Some(body.clone())).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
        let p = v["problems"].to_string();
        assert!(p.contains(word), "{p} should mention {word}");
    }
    // not valid json shapes
    for body in [
        json!({"options": {}}),
        json!({"allowCustomCommands": true, "options": {"zzz": 1}}),
        json!({"allowCustomCommands": true, "options": {"wrappers": ["rm"]}}),
    ] {
        let (st, _) = t.c("PUT", "/api/launch/defaults", Some(body)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }
    assert!(!t.dir.path().join("launch.json").exists());
    // too big a body
    let big = defaults(false, json!({"extra_args": ["x".repeat(6000)]}));
    let (st, _) = t.c("PUT", "/api/launch/defaults", Some(big)).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn override_inherits_per_field_and_effective_is_reported() {
    let t = t();
    let d = json!({"wrappers": ["mangohud"], "env": {"DXVK_HUD": "fps"}, "extra_args": ["-a"]});
    t.c("PUT", "/api/launch/defaults", Some(defaults(false, d)))
        .await;
    let (st, v) = t.c("GET", "/api/games/steam:620/launch", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["override"], Value::Null);
    assert_eq!(v["effective"]["wrappers"], json!(["mangohud"]));
    assert_eq!(v["problems"], json!([]));
    let line = v["steamLine"].as_str().unwrap();
    assert!(line.ends_with(" wrap steam:620 -- %command%"), "{line}");
    // override one field only
    let body = json!({"override": {"wrappers": ["gamemode"]}});
    let (st, v) = t.c("PUT", "/api/games/steam:620/launch", Some(body)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["effective"]["wrappers"], json!(["gamemode"]));
    assert_eq!(
        v["effective"]["env"]["DXVK_HUD"], "fps",
        "env still inherited"
    );
    assert_eq!(v["effective"]["extra_args"], json!(["-a"]));
    // a bare appid is the same game
    let (_, v) = t.c("GET", "/api/games/620/launch", None).await;
    assert_eq!(v["override"]["wrappers"], json!(["gamemode"]));
    // another game is untouched
    let (_, v) = t.c("GET", "/api/games/steam:10/launch", None).await;
    assert_eq!(v["effective"]["wrappers"], json!(["mangohud"]));
    // delete goes back to inheriting
    let (st, v) = t.c("DELETE", "/api/games/620/launch", None).await;
    assert_eq!((st, v["override"].clone()), (StatusCode::OK, Value::Null));
    assert_eq!(v["effective"]["wrappers"], json!(["mangohud"]));
    // emulator ids are accepted and get no steam line
    let (st, v) = t
        .c("GET", "/api/games/emu:dolphin:mario-kart-wii/launch", None)
        .await;
    assert_eq!((st, v["steamLine"].clone()), (StatusCode::OK, Value::Null));
}

#[tokio::test]
async fn bad_ids_and_bad_overrides() {
    let t = t();
    for id in ["steam:0620", "nope", "emu:dolphin", "steam:1;rm"] {
        let (st, _) = t.c("GET", &format!("/api/games/{id}/launch"), None).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{id}");
    }
    let bad = [
        json!({"override": {"env": {"LD_PRELOAD": "x"}}}),
        json!({"override": {"custom": ["tool"]}}), // switch is off
        json!({"override": {"wrappers": [{"gamescope": {"args": ["-W", "99999"]}}]}}),
        json!({"override": {"nope": 1}}),
        json!({"nothing": {}}),
    ];
    for b in bad {
        let (st, _) = t
            .c("PUT", "/api/games/steam:620/launch", Some(b.clone()))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{b}");
    }
    let (_, v) = t.c("GET", "/api/games/steam:620/launch", None).await;
    assert_eq!(v["override"], Value::Null);
}

#[tokio::test]
async fn stale_rev_is_409_with_the_current_state() {
    let t = t();
    let (_, d) = t.c("GET", "/api/launch/defaults", None).await;
    let rev = d["rev"].as_str().unwrap().to_string();
    let b = |r: &str, w: &str| json!({"rev": r, "allowCustomCommands": false, "options": {"wrappers": [w]}});
    let (st, _) = t
        .c("PUT", "/api/launch/defaults", Some(b(&rev, "mangohud")))
        .await;
    assert_eq!(st, StatusCode::OK);
    let (st, v) = t
        .c("PUT", "/api/launch/defaults", Some(b(&rev, "gamemode")))
        .await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(v["current"]["options"]["wrappers"], json!(["mangohud"]));
    // per game
    let (_, g) = t.c("GET", "/api/games/steam:5/launch", None).await;
    let rev = g["rev"].as_str().unwrap().to_string();
    let put = |r: &str| json!({"rev": r, "override": {"wrappers": []}});
    assert_eq!(
        t.c("PUT", "/api/games/steam:5/launch", Some(put(&rev)))
            .await
            .0,
        StatusCode::OK
    );
    let (st, v) = t
        .c("PUT", "/api/games/steam:5/launch", Some(put(&rev)))
        .await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(v["current"]["override"]["wrappers"], json!([]));
}

#[tokio::test]
async fn switching_custom_off_later_shows_problems_for_saved_options() {
    let t = t();
    let body = defaults(true, json!({"custom": ["mytool", "--x"]}));
    assert_eq!(
        t.c("PUT", "/api/launch/defaults", Some(body)).await.0,
        StatusCode::OK
    );
    // a custom command cannot be turned off while it is in use? it can: the options are cleared
    // by the same save. Here the file is edited by hand instead.
    let f = t.dir.path().join("launch.json");
    let mut j: Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
    j["allowCustomCommands"] = json!(false);
    std::fs::write(&f, j.to_string()).unwrap();
    let t2 = {
        let mut cfg = Config::from_env(t.dir.path().to_path_buf());
        cfg.secret = SECRET.into();
        cfg.data_dir = t.dir.path().to_path_buf();
        T {
            app: router(Arc::new(AppState::new(cfg))),
            dir: tempfile::tempdir().unwrap(),
        }
    };
    let (_, v) = t2.c("GET", "/api/games/steam:1/launch", None).await;
    assert!(v["problems"][0].as_str().unwrap().contains("switched off"));
}
